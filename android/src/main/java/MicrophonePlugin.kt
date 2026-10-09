package app.tauri.microphone

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.net.Uri
import android.os.Build
import android.provider.Settings
import app.tauri.Logger
import app.tauri.PermissionState
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import kotlin.math.max

private const val PERMISSION_ALIAS_MICROPHONE = "microphone"
private const val DEFAULT_SAMPLE_RATE = 48_000
private const val FRAME_SAMPLES = 1_024

@InvokeArg
class RecordingOptions {
  var format: String? = null
  var sampleRate: Int? = null
  var channels: Int? = null
  var bitRate: Int? = null
  var maxDuration: Double? = null
  var deviceId: String? = null
  var background: Boolean? = null
}

@InvokeArg
class MeterOptions {
  var intervalMs: Int? = null
  var scale: String? = null
  var floorDb: Double? = null
  var peakHold: Boolean? = null
  var spectrum: Boolean? = null
}

@InvokeArg
class StreamOptions {
  var sampleRate: Int? = null
  var channels: Int? = null
  var frameSamples: Int? = null
  var maxSeconds: Double? = null
}

@InvokeArg
class StreamStartArgs {
  var options: StreamOptions? = null
  lateinit var channel: Channel
}

@TauriPlugin(
  permissions = [
    Permission(strings = [Manifest.permission.RECORD_AUDIO], alias = PERMISSION_ALIAS_MICROPHONE)
  ]
)
class MicrophonePlugin(private val activity: Activity) : Plugin(activity) {
  private val stateMachine = RecordingStateMachine()

  @Volatile private var recording = false
  @Volatile private var paused = false
  private var recorder: AudioRecord? = null
  private var recordingThread: Thread? = null

  private val pcm = ByteArrayOutputStream()
  private var sampleRate = DEFAULT_SAMPLE_RATE
  private var channels = 1
  private var frames = 0L
  private var maxFrames = 0L
  private var outputFile: File? = null
  private var effectiveFormat = "wav"
  private var pendingClip: JSObject? = null

  @Volatile private var meterActive = false
  private var meterOptions = MeterOptions()
  @Volatile private var lastMeterAt = 0L
  private var peakHoldValue = 0.0
  private var meterThread: Thread? = null

  private var mediaRecorder: MediaRecorder? = null
  @Volatile private var usingMediaRecorder = false
  private var mrAccumulatedMs = 0L
  private var mrSegmentStart = 0L

  private var streamRecorder: AudioRecord? = null
  private var streamThread: Thread? = null
  @Volatile private var streaming = false

  private var requestPermissionResponse: JSObject? = null

  // -------------------------------------------------------------------------
  // Recording lifecycle
  // -------------------------------------------------------------------------

  @Command
  fun startRecording(invoke: Invoke) {
    val args = invoke.parseArgs(RecordingOptions::class.java)

    if (getPermissionState(PERMISSION_ALIAS_MICROPHONE) != PermissionState.GRANTED) {
      invoke.reject("No permission to use the microphone. Did you request it yet?")
      return
    }
    try {
      stateMachine.start()
    } catch (e: IllegalArgumentException) {
      invoke.reject(e.message ?: "Cannot start recording")
      return
    }

    sampleRate = args.sampleRate ?: DEFAULT_SAMPLE_RATE
    channels = if ((args.channels ?: 1) >= 2) 2 else 1
    // Opus needs API 29+; otherwise degrade to WAV and report the real format.
    effectiveFormat = AudioMath.effectiveFormat(args.format)
    if (effectiveFormat == "opus" && !AudioMath.supportsOpus(Build.VERSION.SDK_INT)) {
      effectiveFormat = "wav"
    }
    outputFile = File(
      activity.cacheDir,
      "tauri-microphone-${System.currentTimeMillis()}.${AudioMath.effectiveExtension(effectiveFormat)}"
    )
    pcm.reset()
    frames = 0L
    paused = false
    pendingClip = null
    maxFrames = if ((args.maxDuration ?: 0.0) > 0.0) {
      ((args.maxDuration ?: 0.0) * sampleRate).toLong()
    } else {
      0L
    }

    if (AudioMath.usesMediaRecorder(effectiveFormat)) {
      try {
        startMediaRecorder(args, effectiveFormat)
      } catch (e: Exception) {
        try {
          stateMachine.stop()
        } catch (_: IllegalArgumentException) {
        }
        invoke.reject("Failed to start ${effectiveFormat.uppercase()} recording: ${e.message}")
        return
      }
    } else {
      recording = true
      startCapture()
    }
    if (args.background == true) {
      MicrophoneForegroundService.start(activity)
    }
    invoke.resolve()
  }

  private fun startMediaRecorder(options: RecordingOptions, format: String) {
    @Suppress("DEPRECATION")
    val recorder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
      MediaRecorder(activity)
    } else {
      MediaRecorder()
    }
    val isOpus = format == "opus"
    recorder.setAudioSource(MediaRecorder.AudioSource.MIC)
    recorder.setOutputFormat(
      if (isOpus) MediaRecorder.OutputFormat.OGG else MediaRecorder.OutputFormat.MPEG_4
    )
    recorder.setAudioEncoder(
      if (isOpus) MediaRecorder.AudioEncoder.OPUS else MediaRecorder.AudioEncoder.AAC
    )
    recorder.setAudioEncodingBitRate(options.bitRate ?: if (isOpus) 96_000 else 128_000)
    recorder.setAudioSamplingRate(sampleRate)
    recorder.setAudioChannels(channels)
    recorder.setOutputFile(outputFile!!.absolutePath)
    recorder.prepare()
    recorder.start()
    mediaRecorder = recorder
    usingMediaRecorder = true
    mrAccumulatedMs = 0L
    mrSegmentStart = System.currentTimeMillis()
  }

  private fun currentMediaRecorderDuration(): Double {
    var millis = mrAccumulatedMs
    if (mrSegmentStart > 0L) millis += System.currentTimeMillis() - mrSegmentStart
    return millis / 1000.0
  }

  private fun startCapture() {
    val channelMask =
      if (channels == 2) AudioFormat.CHANNEL_IN_STEREO else AudioFormat.CHANNEL_IN_MONO
    val minBuffer = AudioRecord.getMinBufferSize(
      sampleRate,
      channelMask,
      AudioFormat.ENCODING_PCM_16BIT
    )
    val bufferSize = max(minBuffer, FRAME_SAMPLES * channels * 2 * 2)
    val rec = AudioRecord(
      MediaRecorder.AudioSource.MIC,
      sampleRate,
      channelMask,
      AudioFormat.ENCODING_PCM_16BIT,
      bufferSize
    )
    recorder = rec

    recordingThread = Thread {
      val buffer = ShortArray(FRAME_SAMPLES * channels)
      val bytes = ByteArray(buffer.size * 2)
      try {
        rec.startRecording()
        while (recording) {
          if (paused) {
            Thread.sleep(15)
            continue
          }
          val read = rec.read(buffer, 0, buffer.size)
          if (read > 0) {
            var bi = 0
            for (i in 0 until read) {
              val sample = buffer[i].toInt()
              bytes[bi++] = (sample and 0xff).toByte()
              bytes[bi++] = ((sample shr 8) and 0xff).toByte()
            }
            synchronized(pcm) { pcm.write(bytes, 0, read * 2) }
            frames += (read / channels).toLong()
            if (meterActive) emitMeter(buffer, read)
            if (maxFrames > 0 && frames >= maxFrames) {
              recording = false
              pendingClip = finalizeFile()
              try {
                stateMachine.stop()
              } catch (_: IllegalArgumentException) {
              }
              break
            }
          }
        }
      } catch (e: Exception) {
        Logger.error("Microphone capture failed: ${e.message}")
      } finally {
        try {
          rec.stop()
        } catch (_: Exception) {
        }
        rec.release()
      }
    }
    recordingThread?.start()
  }

  private fun finalizeFile(): JSObject? {
    val out = outputFile ?: return null
    val data: ByteArray
    synchronized(pcm) { data = pcm.toByteArray() }
    try {
      FileOutputStream(out).use { stream ->
        stream.write(AudioMath.wavHeader(data.size, sampleRate, channels))
        stream.write(data)
      }
    } catch (e: Exception) {
      Logger.error("Failed to write WAV file: ${e.message}")
      return null
    }
    val clip = JSObject()
    clip.put("path", out.absolutePath)
    clip.put("duration", AudioMath.durationSeconds(frames, sampleRate))
    clip.put("format", effectiveFormat)
    return clip
  }

  @Command
  fun stopRecording(invoke: Invoke) {
    pendingClip?.let { clip ->
      pendingClip = null
      invoke.resolve(clip)
      return
    }
    if (stateMachine.state == RecordingStateMachine.IDLE) {
      invoke.reject("cannot stop while idle")
      return
    }
    MicrophoneForegroundService.stop(activity)

    if (usingMediaRecorder) {
      val duration = currentMediaRecorderDuration()
      meterActive = false
      meterThread?.interrupt()
      try {
        mediaRecorder?.stop()
      } catch (_: Exception) {
      }
      mediaRecorder?.release()
      mediaRecorder = null
      usingMediaRecorder = false
      try {
        stateMachine.stop()
      } catch (_: IllegalArgumentException) {
      }
      val clip = JSObject()
      clip.put("path", outputFile!!.absolutePath)
      clip.put("duration", duration)
      clip.put("format", effectiveFormat)
      invoke.resolve(clip)
      return
    }

    recording = false
    try {
      recordingThread?.join(2_000)
    } catch (_: InterruptedException) {
    }
    recordingThread = null
    recorder = null
    val clip = finalizeFile()
    try {
      stateMachine.stop()
    } catch (_: IllegalArgumentException) {
    }
    if (clip == null) {
      invoke.reject("Failed to finalize the recording")
    } else {
      invoke.resolve(clip)
    }
  }

  @Command
  fun pauseRecording(invoke: Invoke) {
    try {
      stateMachine.pause()
    } catch (e: IllegalArgumentException) {
      invoke.reject(e.message ?: "Cannot pause")
      return
    }
    if (usingMediaRecorder) {
      try {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) mediaRecorder?.pause()
      } catch (_: Exception) {
      }
      mrAccumulatedMs += System.currentTimeMillis() - mrSegmentStart
      mrSegmentStart = 0L
      invoke.resolve()
      return
    }
    paused = true
    invoke.resolve()
  }

  @Command
  fun resumeRecording(invoke: Invoke) {
    try {
      stateMachine.resume()
    } catch (e: IllegalArgumentException) {
      invoke.reject(e.message ?: "Cannot resume")
      return
    }
    if (usingMediaRecorder) {
      try {
        mediaRecorder?.resume()
      } catch (_: Exception) {
      }
      mrSegmentStart = System.currentTimeMillis()
      invoke.resolve()
      return
    }
    paused = false
    invoke.resolve()
  }

  @Command
  fun getRecordingState(invoke: Invoke) {
    invoke.resolveObject(stateMachine.state)
  }

  // -------------------------------------------------------------------------
  // Capabilities
  // -------------------------------------------------------------------------

  @Command
  fun getCapabilities(invoke: Invoke) {
    val formats = AudioMath.supportedFormats(Build.VERSION.SDK_INT)
    val result = JSObject()
    result.put("formats", formats)
    result.put("preferredFormat", "wav")
    result.put("sampleRates", listOf(44_100, 48_000))
    result.put("channels", listOf(1, 2))
    result.put("supportsPause", true)
    result.put("supportsBackground", true)
    result.put("supportsSpectrum", true)
    result.put("supportsAudioStream", true)
    result.put("streamTransport", "base64")
    invoke.resolve(result)
  }

  @Command
  fun getSupportedFormats(invoke: Invoke) {
    invoke.resolveObject(AudioMath.supportedFormats(Build.VERSION.SDK_INT))
  }

  // -------------------------------------------------------------------------
  // Raw audio streaming
  // -------------------------------------------------------------------------

  @Command
  fun startAudioStream(invoke: Invoke) {
    val args = invoke.parseArgs(StreamStartArgs::class.java)
    if (getPermissionState(PERMISSION_ALIAS_MICROPHONE) != PermissionState.GRANTED) {
      invoke.reject("No permission to use the microphone. Did you request it yet?")
      return
    }
    if (streaming) {
      invoke.reject("an audio stream is already active")
      return
    }
    val options = args.options ?: StreamOptions()
    val streamSampleRate = options.sampleRate ?: DEFAULT_SAMPLE_RATE
    val streamChannels = if ((options.channels ?: 1) >= 2) 2 else 1
    val frameSamples = options.frameSamples ?: FRAME_SAMPLES
    val channel = args.channel

    val channelMask =
      if (streamChannels == 2) AudioFormat.CHANNEL_IN_STEREO else AudioFormat.CHANNEL_IN_MONO
    val minBuffer = AudioRecord.getMinBufferSize(
      streamSampleRate,
      channelMask,
      AudioFormat.ENCODING_PCM_16BIT
    )
    val bufferSize = max(minBuffer, frameSamples * streamChannels * 2 * 2)
    val recorder = AudioRecord(
      MediaRecorder.AudioSource.MIC,
      streamSampleRate,
      channelMask,
      AudioFormat.ENCODING_PCM_16BIT,
      bufferSize
    )
    streamRecorder = recorder
    streaming = true
    val maxFrames = if ((options.maxSeconds ?: 0.0) > 0.0) {
      ((options.maxSeconds ?: 0.0) * streamSampleRate).toLong()
    } else {
      0L
    }

    streamThread = Thread {
      val buffer = ShortArray(frameSamples * streamChannels)
      var sequence = 0L
      var framesWritten = 0L
      val start = System.currentTimeMillis()
      try {
        recorder.startRecording()
        while (streaming && !Thread.currentThread().isInterrupted) {
          val read = recorder.read(buffer, 0, buffer.size)
          if (read > 0) {
            val data = JSObject()
            data.put("sequence", sequence)
            data.put("timestamp", (System.currentTimeMillis() - start) / 1000.0)
            data.put("data", AudioMath.base64(AudioMath.pcmToBytes(buffer, read)))
            channel.send(data)
            sequence++
            framesWritten += (read / streamChannels).toLong()
            if (maxFrames > 0 && framesWritten >= maxFrames) break
          }
        }
      } catch (e: Exception) {
        Logger.error("Audio stream failed: ${e.message}")
      } finally {
        try {
          recorder.stop()
        } catch (_: Exception) {
        }
        recorder.release()
      }
    }
    streamThread?.start()

    val info = JSObject()
    info.put("transport", "base64")
    info.put("sampleRate", streamSampleRate)
    info.put("channels", streamChannels)
    info.put("frameSamples", frameSamples)
    invoke.resolve(info)
  }

  @Command
  fun stopAudioStream(invoke: Invoke) {
    streaming = false
    streamThread?.interrupt()
    try {
      streamThread?.join(1_000)
    } catch (_: InterruptedException) {
    }
    streamThread = null
    streamRecorder = null
    invoke.resolve()
  }

  // -------------------------------------------------------------------------
  // Metering
  // -------------------------------------------------------------------------

  @Command
  fun startMeter(invoke: Invoke) {
    meterOptions = invoke.parseArgs(MeterOptions::class.java)
    meterActive = true
    lastMeterAt = 0L
    peakHoldValue = 0.0
    if (usingMediaRecorder) startMediaRecorderMeter()
    invoke.resolve()
  }

  @Command
  fun stopMeter(invoke: Invoke) {
    meterActive = false
    meterThread?.interrupt()
    meterThread = null
    invoke.resolve()
  }

  private fun startMediaRecorderMeter() {
    meterThread?.interrupt()
    meterThread = Thread {
      while (meterActive && usingMediaRecorder) {
        val recorder = mediaRecorder ?: break
        val amplitude = try {
          recorder.maxAmplitude
        } catch (_: Exception) {
          0
        }
        val floor = meterOptions.floorDb ?: -60.0
        val peakLinear = AudioMath.linearLevel(amplitude)
        val held = if (meterOptions.peakHold == true) {
          AudioMath.applyPeakHold(peakHoldValue, peakLinear)
        } else {
          peakLinear
        }
        peakHoldValue = held
        val scale = meterOptions.scale ?: "linear"
        val level = if (scale == "dbfs") AudioMath.dbfs(peakLinear, floor) else peakLinear
        val data = JSObject()
        data.put("level", level)
        data.put("peak", held)
        data.put("db", AudioMath.dbfs(peakLinear, floor))
        data.put("clipping", peakLinear >= 1.0)
        data.put("timestamp", System.currentTimeMillis() / 1000.0)
        trigger("meter", data)
        try {
          Thread.sleep((meterOptions.intervalMs ?: 100).toLong())
        } catch (_: InterruptedException) {
          break
        }
      }
    }
    meterThread?.start()
  }

  private fun emitMeter(buffer: ShortArray, read: Int) {
    val now = System.currentTimeMillis()
    val interval = meterOptions.intervalMs ?: 100
    if (now - lastMeterAt < interval) return
    lastMeterAt = now

    val floor = meterOptions.floorDb ?: -60.0
    val peakLinear = AudioMath.linearLevel(AudioMath.peakAmplitude(buffer, read))
    val rmsLinear = AudioMath.linearLevel(AudioMath.rmsAmplitude(buffer, read).toInt())
    val held = if (meterOptions.peakHold == true) {
      AudioMath.applyPeakHold(peakHoldValue, peakLinear)
    } else {
      peakLinear
    }
    peakHoldValue = held
    val scale = meterOptions.scale ?: "linear"
    val level = if (scale == "dbfs") AudioMath.dbfs(peakLinear, floor) else peakLinear

    val data = JSObject()
    data.put("level", level)
    data.put("peak", held)
    data.put("rms", rmsLinear)
    data.put("db", AudioMath.dbfs(peakLinear, floor))
    data.put("clipping", peakLinear >= 1.0)
    if (meterOptions.spectrum == true) {
      data.put("spectrum", AudioMath.magnitudeSpectrum(buffer, read).toList())
    }
    data.put("timestamp", now / 1000.0)
    trigger("meter", data)
  }

  // -------------------------------------------------------------------------
  // Permissions
  // -------------------------------------------------------------------------

  @SuppressLint("ObsoleteSdkInt")
  private fun permissionState(): PermissionState {
    val state = AudioMath.permissionState(
      granted = getPermissionState(PERMISSION_ALIAS_MICROPHONE) == PermissionState.GRANTED,
      requestable = Build.VERSION.SDK_INT >= Build.VERSION_CODES.M
    )
    return PermissionState.valueOf(state.uppercase())
  }

  @Command
  override fun checkPermissions(invoke: Invoke) {
    val result = JSObject()
    result.put("microphone", permissionState().toString())
    invoke.resolve(result)
  }

  @SuppressLint("ObsoleteSdkInt")
  @Command
  override fun requestPermissions(invoke: Invoke) {
    val response = JSObject()
    requestPermissionResponse = response
    if (getPermissionState(PERMISSION_ALIAS_MICROPHONE) == PermissionState.GRANTED) {
      response.put("microphone", PermissionState.GRANTED.toString())
      invoke.resolve(response)
      requestPermissionResponse = null
    } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
      requestPermissionForAlias(PERMISSION_ALIAS_MICROPHONE, invoke, "microphonePermissionCallback")
    } else {
      response.put("microphone", PermissionState.GRANTED.toString())
      invoke.resolve(response)
      requestPermissionResponse = null
    }
  }

  @SuppressLint("ObsoleteSdkInt")
  @PermissionCallback
  fun microphonePermissionCallback(invoke: Invoke) {
    val response = requestPermissionResponse ?: JSObject()
    response.put("microphone", permissionState().toString())
    invoke.resolve(response)
    requestPermissionResponse = null
  }

  @Command
  fun openAppSettings(invoke: Invoke) {
    val intent = Intent(
      Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
      Uri.fromParts("package", activity.packageName, null)
    )
    intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    startActivityForResult(invoke, intent, "openSettingsResult")
  }

  @Suppress("UNUSED_PARAMETER")
  @ActivityCallback
  private fun openSettingsResult(invoke: Invoke, activityResult: androidx.activity.result.ActivityResult) {
    invoke.resolve()
  }
}

package app.tauri.microphone

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.log10
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Pure audio helpers. Kept free of Android framework dependencies so they can be
 * unit-tested with plain JUnit (no microphone, emulator or device required).
 */
internal object AudioMath {

  const val BITS_PER_SAMPLE = 16

  /** Bytes for `samples` frames across `channels`. */
  fun pcmByteCount(samples: Int, channels: Int): Int =
    samples * channels * (BITS_PER_SAMPLE / 8)

  /** Seconds represented by `frames` at `sampleRate`. */
  fun durationSeconds(frames: Long, sampleRate: Int): Double =
    if (sampleRate <= 0) 0.0 else frames.toDouble() / sampleRate.toDouble()

  /**
   * Builds a canonical 44-byte PCM WAV header for the given payload size.
   */
  fun wavHeader(
    dataSize: Int,
    sampleRate: Int,
    channels: Int,
    bitsPerSample: Int = BITS_PER_SAMPLE
  ): ByteArray {
    val blockAlign = channels * bitsPerSample / 8
    val byteRate = sampleRate * blockAlign
    val header = ByteArray(44)

    fun ascii(offset: Int, value: String) {
      for (i in value.indices) header[offset + i] = value[i].code.toByte()
    }
    fun intLE(offset: Int, value: Int) {
      header[offset] = (value and 0xff).toByte()
      header[offset + 1] = ((value ushr 8) and 0xff).toByte()
      header[offset + 2] = ((value ushr 16) and 0xff).toByte()
      header[offset + 3] = ((value ushr 24) and 0xff).toByte()
    }
    fun shortLE(offset: Int, value: Int) {
      header[offset] = (value and 0xff).toByte()
      header[offset + 1] = ((value ushr 8) and 0xff).toByte()
    }

    ascii(0, "RIFF")
    intLE(4, 36 + dataSize)
    ascii(8, "WAVE")
    ascii(12, "fmt ")
    intLE(16, 16)
    shortLE(20, 1) // PCM
    shortLE(22, channels)
    intLE(24, sampleRate)
    intLE(28, byteRate)
    shortLE(32, blockAlign)
    shortLE(34, bitsPerSample)
    ascii(36, "data")
    intLE(40, dataSize)
    return header
  }

  /**
   * Applies a peak-hold with decay: keeps the previous peak unless the current
   * level is higher, decaying it by `decay` on each step.
   */
  fun applyPeakHold(previous: Double, current: Double, decay: Double = 0.9): Double =
    kotlin.math.max(current, previous * decay)

  /**
   * Coarse magnitude spectrum: `bands` DFT magnitudes sampled across the
   * positive frequency range, normalized to 0..1.
   */
  fun magnitudeSpectrum(buffer: ShortArray, read: Int, bands: Int = 16): DoubleArray {
    if (read <= 0 || bands <= 0) return DoubleArray(0)
    val out = DoubleArray(bands)
    val half = read / 2
    for (b in 0 until bands) {
      val k = ((b + 1) * half / bands).coerceAtLeast(1)
      var re = 0.0
      var im = 0.0
      for (t in 0 until read) {
        val angle = 2.0 * PI * k * t / read
        val sample = buffer[t].toDouble()
        re += sample * kotlin.math.cos(angle)
        im -= sample * kotlin.math.sin(angle)
      }
      out[b] = (sqrt(re * re + im * im) / read / 32_768.0).coerceIn(0.0, 1.0)
    }
    return out
  }

  /** Maps a peak sample amplitude to a linear level in 0..1. */
  fun linearLevel(peak: Int, maxAmplitude: Int = 32_767): Double =
    if (maxAmplitude <= 0) 0.0 else (peak.toDouble() / maxAmplitude).coerceIn(0.0, 1.0)

  /** dBFS for a linear level, clamped to `floorDb`..0. */
  fun dbfs(level: Double, floorDb: Double = -60.0): Double {
    if (level <= 0.0) return floorDb
    val db = 20.0 * log10(level)
    return db.coerceIn(floorDb, 0.0)
  }

  /** Peak absolute amplitude of a PCM buffer. */
  fun peakAmplitude(buffer: ShortArray, read: Int): Int {
    var peak = 0
    for (i in 0 until read) {
      val v = kotlin.math.abs(buffer[i].toInt())
      if (v > peak) peak = v
    }
    return peak
  }

  /** RMS amplitude of a PCM buffer. */
  fun rmsAmplitude(buffer: ShortArray, read: Int): Double {
    if (read <= 0) return 0.0
    var sum = 0.0
    for (i in 0 until read) {
      val v = buffer[i].toDouble()
      sum += v * v
    }
    return sqrt(sum / read)
  }

  /** Formats guaranteed/progressive on the given Android API level. */
  fun supportedFormats(apiLevel: Int): List<String> {
    val formats = mutableListOf("wav", "m4a", "aac", "flac")
    if (apiLevel >= 29) formats.add("opus")
    return formats
  }

  /**
   * Maps an OS permission probe to the plugin permission state.
   *
   * `granted` means the microphone permission is held; `requestable` means the
   * OS can still show a prompt (Android M+); otherwise it is permanently denied.
   */
  fun permissionState(granted: Boolean, requestable: Boolean): String = when {
    granted -> "granted"
    requestable -> "prompt"
    else -> "denied"
  }

  private const val BASE64_ALPHABET =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

  /** Converts interleaved 16-bit PCM samples to little-endian bytes. */
  fun pcmToBytes(buffer: ShortArray, read: Int): ByteArray {
    val out = ByteArray(read * 2)
    for (i in 0 until read) {
      val sample = buffer[i].toInt()
      out[i * 2] = (sample and 0xff).toByte()
      out[i * 2 + 1] = ((sample shr 8) and 0xff).toByte()
    }
    return out
  }

  /** Standard Base64 encoding (no line breaks). */
  fun base64(bytes: ByteArray): String {
    val builder = StringBuilder((bytes.size + 2) / 3 * 4)
    var i = 0
    while (i < bytes.size) {
      val b0 = bytes[i].toInt() and 0xff
      val b1 = if (i + 1 < bytes.size) bytes[i + 1].toInt() and 0xff else 0
      val b2 = if (i + 2 < bytes.size) bytes[i + 2].toInt() and 0xff else 0
      val triple = (b0 shl 16) or (b1 shl 8) or b2
      builder.append(BASE64_ALPHABET[(triple shr 18) and 0x3f])
      builder.append(BASE64_ALPHABET[(triple shr 12) and 0x3f])
      builder.append(if (i + 1 < bytes.size) BASE64_ALPHABET[(triple shr 6) and 0x3f] else '=')
      builder.append(if (i + 2 < bytes.size) BASE64_ALPHABET[triple and 0x3f] else '=')
      i += 3
    }
    return builder.toString()
  }

  /** Whether the requested format is produced through the AAC/MP4 path. */
  fun usesAacContainer(format: String?): Boolean = format == "m4a" || format == "aac"

  /** Whether the format is produced by `MediaRecorder` (M4A/AAC/Opus). */
  fun usesMediaRecorder(format: String?): Boolean =
    format == "m4a" || format == "aac" || format == "opus"

  /** Whether Opus is available on the given API level (encoder added in API 29). */
  fun supportsOpus(apiLevel: Int): Boolean = apiLevel >= 29

  /**
   * Resolves the format to capture: the requested one when it is supported by
   * the platform, otherwise `wav` (never a mislabeled container).
   */
  fun resolveFormat(requested: String?, supported: List<String>): String =
    if (requested != null && supported.contains(requested)) requested else "wav"

  /** Effective format reported for a requested one. */
  fun effectiveFormat(format: String?): String = when (format) {
    "m4a" -> "m4a"
    "aac" -> "aac"
    "flac" -> "flac"
    "opus" -> "opus"
    else -> "wav"
  }

  /** File extension for a requested format, degrading unknown formats to wav. */
  fun effectiveExtension(format: String?): String = when (format) {
    "m4a" -> "m4a"
    "aac" -> "aac"
    "opus" -> "opus"
    "flac" -> "flac"
    else -> "wav"
  }
}

/**
 * Pure recording state machine: `idle -> recording -> paused -> recording -> idle`.
 * Invalid transitions throw with a descriptive message.
 */
internal class RecordingStateMachine {
  var state: String = IDLE
    private set

  fun start() {
    require(state == IDLE) { "cannot start while $state" }
    state = RECORDING
  }

  fun stop() {
    require(state != IDLE) { "cannot stop while idle" }
    state = IDLE
  }

  fun pause() {
    require(state == RECORDING) { "cannot pause while $state" }
    state = PAUSED
  }

  fun resume() {
    require(state == PAUSED) { "cannot resume while $state" }
    state = RECORDING
  }

  companion object {
    const val IDLE = "idle"
    const val RECORDING = "recording"
    const val PAUSED = "paused"
  }
}

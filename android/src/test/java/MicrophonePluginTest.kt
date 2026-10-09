package app.tauri.microphone

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MicrophonePluginTest {

  @Test
  fun wavHeaderIsCanonicalPcm() {
    val header = AudioMath.wavHeader(dataSize = 1000, sampleRate = 48000, channels = 1)
    assertEquals(44, header.size)
    assertEquals("RIFF", String(header, 0, 4))
    assertEquals("WAVE", String(header, 8, 4))
    assertEquals("fmt ", String(header, 12, 4))
    assertEquals("data", String(header, 36, 4))
    // data chunk size at offset 40 (little-endian).
    val dataSize = (header[40].toInt() and 0xff) or
      ((header[41].toInt() and 0xff) shl 8) or
      ((header[42].toInt() and 0xff) shl 16) or
      ((header[43].toInt() and 0xff) shl 24)
    assertEquals(1000, dataSize)
    // RIFF size = 36 + data size.
    val riffSize = (header[4].toInt() and 0xff) or
      ((header[5].toInt() and 0xff) shl 8) or
      ((header[6].toInt() and 0xff) shl 16) or
      ((header[7].toInt() and 0xff) shl 24)
    assertEquals(1036, riffSize)
  }

  @Test
  fun wavHeaderReportsChannelsAndRate() {
    val header = AudioMath.wavHeader(dataSize = 0, sampleRate = 44100, channels = 2)
    val channels = (header[22].toInt() and 0xff) or ((header[23].toInt() and 0xff) shl 8)
    val rate = (header[24].toInt() and 0xff) or
      ((header[25].toInt() and 0xff) shl 8) or
      ((header[26].toInt() and 0xff) shl 16) or
      ((header[27].toInt() and 0xff) shl 24)
    assertEquals(2, channels)
    assertEquals(44100, rate)
  }

  @Test
  fun linearLevelMapsToZeroOne() {
    assertEquals(0.0, AudioMath.linearLevel(0), 0.0001)
    assertEquals(0.5, AudioMath.linearLevel(16384), 0.01)
    assertEquals(1.0, AudioMath.linearLevel(32767), 0.0001)
    assertEquals(1.0, AudioMath.linearLevel(99999), 0.0001)
  }

  @Test
  fun dbfsIsClampedToFloor() {
    assertEquals(0.0, AudioMath.dbfs(1.0), 0.0001)
    assertEquals(-60.0, AudioMath.dbfs(0.0), 0.0001)
    assertEquals(-60.0, AudioMath.dbfs(0.000001), 0.0001)
    assertTrue(AudioMath.dbfs(0.5) < 0.0)
  }

  @Test
  fun peakAndRmsFromBuffer() {
    val buffer = shortArrayOf(100, -32768, 300, -200)
    assertEquals(32768, AudioMath.peakAmplitude(buffer, 4))
    assertTrue(AudioMath.rmsAmplitude(buffer, 4) > 0.0)
    assertEquals(0.0, AudioMath.rmsAmplitude(buffer, 0), 0.0001)
  }

  @Test
  fun peakHoldKeepsHigherAndDecays() {
    assertEquals(0.5, AudioMath.applyPeakHold(0.4, 0.5), 0.0001)
    assertEquals(0.36, AudioMath.applyPeakHold(0.4, 0.1), 0.0001)
  }

  @Test
  fun magnitudeSpectrumReturnsNormalizedBands() {
    val buffer = ShortArray(1024) { (kotlin.math.sin(it.toDouble()) * 1000).toInt().toShort() }
    val spectrum = AudioMath.magnitudeSpectrum(buffer, 1024, bands = 8)
    assertEquals(8, spectrum.size)
    assertTrue(spectrum.all { it >= 0.0 && it <= 1.0 })
  }

  @Test
  fun durationUsesSampleRate() {
    assertEquals(1.0, AudioMath.durationSeconds(48000, 48000), 0.0001)
    assertEquals(0.0, AudioMath.durationSeconds(48000, 0), 0.0001)
  }

  @Test
  fun supportedFormatsDependOnApiLevel() {
    assertArrayEquals(
      arrayOf("wav", "m4a", "aac"),
      AudioMath.supportedFormats(28).toTypedArray()
    )
    assertTrue(AudioMath.supportedFormats(29).contains("opus"))
  }

  @Test
  fun permissionStateMapping() {
    assertEquals("granted", AudioMath.permissionState(granted = true, requestable = false))
    assertEquals("prompt", AudioMath.permissionState(granted = false, requestable = true))
    assertEquals("denied", AudioMath.permissionState(granted = false, requestable = false))
  }

  @Test
  fun unknownFormatDegradesToWavExtension() {
    assertEquals("wav", AudioMath.effectiveExtension(null))
    assertEquals("wav", AudioMath.effectiveExtension("unknown"))
    assertEquals("m4a", AudioMath.effectiveExtension("m4a"))
  }

  @Test
  fun pcmToBytesIsLittleEndian() {
    val buffer = shortArrayOf(1, -2, 258)
    val bytes = AudioMath.pcmToBytes(buffer, 3)
    assertArrayEquals(
      byteArrayOf(1, 0, 0xFE.toByte(), 0xFF.toByte(), 0x02, 0x01),
      bytes
    )
  }

  @Test
  fun base64MatchesJvmEncoder() {
    val samples = byteArrayOf(0, 1, 2, 3, 4, 5, 6, 250.toByte())
    assertArrayEquals(samples, java.util.Base64.getDecoder().decode(AudioMath.base64(samples)))
  }

  @Test
  fun base64HandlesPadding() {
    assertArrayEquals(
      byteArrayOf(7),
      java.util.Base64.getDecoder().decode(AudioMath.base64(byteArrayOf(7)))
    )
    assertArrayEquals(
      byteArrayOf(7, 8),
      java.util.Base64.getDecoder().decode(AudioMath.base64(byteArrayOf(7, 8)))
    )
  }

  @Test
  fun aacContainerDetectionAndEffectiveFormat() {
    assertTrue(AudioMath.usesAacContainer("m4a"))
    assertTrue(AudioMath.usesAacContainer("aac"))
    assertTrue(!AudioMath.usesAacContainer("wav"))
    assertTrue(!AudioMath.usesAacContainer(null))
    assertEquals("m4a", AudioMath.effectiveFormat("m4a"))
    assertEquals("aac", AudioMath.effectiveFormat("aac"))
    assertEquals("wav", AudioMath.effectiveFormat("unknown"))
  }

  @Test
  fun mediaRecorderFormatSelection() {
    assertTrue(AudioMath.usesMediaRecorder("m4a"))
    assertTrue(AudioMath.usesMediaRecorder("aac"))
    assertTrue(AudioMath.usesMediaRecorder("opus"))
    assertTrue(!AudioMath.usesMediaRecorder("wav"))
    assertTrue(!AudioMath.usesMediaRecorder(null))
  }

  @Test
  fun opusSupportDependsOnApiLevel() {
    assertTrue(!AudioMath.supportsOpus(28))
    assertTrue(AudioMath.supportsOpus(29))
    assertTrue(AudioMath.supportsOpus(34))
  }

  @Test
  fun stateMachineAllowsValidLifecycle() {
    val sm = RecordingStateMachine()
    assertEquals(RecordingStateMachine.IDLE, sm.state)
    sm.start()
    assertEquals(RecordingStateMachine.RECORDING, sm.state)
    sm.pause()
    assertEquals(RecordingStateMachine.PAUSED, sm.state)
    sm.resume()
    assertEquals(RecordingStateMachine.RECORDING, sm.state)
    sm.stop()
    assertEquals(RecordingStateMachine.IDLE, sm.state)
  }

  @Test(expected = IllegalArgumentException::class)
  fun stateMachineRejectsStopWhileIdle() {
    RecordingStateMachine().stop()
  }

  @Test(expected = IllegalArgumentException::class)
  fun stateMachineRejectsStartWhileRecording() {
    val sm = RecordingStateMachine()
    sm.start()
    sm.start()
  }

  @Test(expected = IllegalArgumentException::class)
  fun stateMachineRejectsPauseWhileIdle() {
    RecordingStateMachine().pause()
  }
}

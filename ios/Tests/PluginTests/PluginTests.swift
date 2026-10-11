import XCTest

@testable import tauri_plugin_microphone

final class MicrophonePluginTests: XCTestCase {

  func testDecodesRecordingOptions() throws {
    let data = Data(
      #"{"format":"flac","sampleRate":44100,"channels":2,"maxDuration":10.5,"deviceId":"mic-1"}"#
        .utf8)
    let options = try JSONDecoder().decode(RecordingOptions.self, from: data)
    XCTAssertEqual(options.format, "flac")
    XCTAssertEqual(options.sampleRate, 44100)
    XCTAssertEqual(options.channels, 2)
    XCTAssertEqual(options.maxDuration, 10.5)
    XCTAssertEqual(options.deviceId, "mic-1")
  }

  func testDecodesMeterOptions() throws {
    let data = Data(#"{"intervalMs":50,"scale":"dbfs","floorDb":-70,"peakHold":true}"#.utf8)
    let options = try JSONDecoder().decode(MeterOptions.self, from: data)
    XCTAssertEqual(options.intervalMs, 50)
    XCTAssertEqual(options.scale, "dbfs")
    XCTAssertEqual(options.floorDb, -70)
    XCTAssertEqual(options.peakHold, true)
  }

  func testLinearLevelFromDb() {
    XCTAssertEqual(AudioMath.linearLevel(fromDb: 0), 1.0, accuracy: 0.0001)
    XCTAssertEqual(AudioMath.linearLevel(fromDb: -60), 0.0, accuracy: 0.0001)
    XCTAssertEqual(AudioMath.linearLevel(fromDb: -100), 0.0, accuracy: 0.0001)
  }

  func testDbfsClampedToFloor() {
    XCTAssertEqual(AudioMath.dbfs(level: 1.0), 0.0, accuracy: 0.0001)
    XCTAssertEqual(AudioMath.dbfs(level: 0.0), -60.0, accuracy: 0.0001)
    XCTAssertTrue(AudioMath.dbfs(level: 0.5) < 0.0)
  }

  func testPeakHoldKeepsHigherAndDecays() {
    XCTAssertEqual(AudioMath.applyPeakHold(previous: 0.4, current: 0.5), 0.5, accuracy: 0.0001)
    XCTAssertEqual(AudioMath.applyPeakHold(previous: 0.4, current: 0.1), 0.36, accuracy: 0.0001)
  }

  func testPermissionStateMapping() {
    XCTAssertEqual(AudioMath.permissionState(granted: true, requestable: false), "granted")
    XCTAssertEqual(AudioMath.permissionState(granted: false, requestable: true), "prompt")
    XCTAssertEqual(AudioMath.permissionState(granted: false, requestable: false), "denied")
  }

  func testSupportedFormatsIncludeWav() {
    XCTAssertTrue(AudioMath.supportedFormats().contains("wav"))
  }

  func testSupportedFormatsIncludeFlac() {
    XCTAssertTrue(AudioMath.supportedFormats().contains("flac"))
    XCTAssertEqual(
      AudioMath.resolveFormat("flac", supported: AudioMath.supportedFormats()), "flac")
  }

  func testFlacDegradesWhenEncoderUnavailable() {
    let supported = AudioMath.supportedFormats()
    XCTAssertEqual(
      AudioMath.resolveFormat("flac", supported: supported, flacEncoderAvailable: false),
      "wav")
    XCTAssertEqual(
      AudioMath.resolveFormat("flac", supported: supported, flacEncoderAvailable: true),
      "flac")
  }

  func testFlacResolutionMatchesEncoderAvailability() {
    let resolved = AudioMath.resolveFormat(
      "flac", supported: AudioMath.supportedFormats(),
      flacEncoderAvailable: FlacEncoder.available)
    XCTAssertEqual(resolved, FlacEncoder.available ? "flac" : "wav")
  }

  func testInt16LittleEndianBytes() {
    let bytes = AudioMath.int16ToLittleEndianBytes([1, -2, 258])
    XCTAssertEqual(bytes, [1, 0, 0xFE, 0xFF, 0x02, 0x01])
    XCTAssertEqual(
      Data(bytes).base64EncodedString(),
      Data([1, 0, 0xFE, 0xFF, 0x02, 0x01]).base64EncodedString()
    )
  }

  func testDecodesStreamOptions() throws {
    let options = try JSONDecoder().decode(
      StreamOptions.self, from: Data(#"{"frameSamples":512,"maxSeconds":3}"#.utf8))
    XCTAssertEqual(options.frameSamples, 512)
    XCTAssertEqual(options.maxSeconds, 3)
  }

  func testAacContainerAndEffectiveFormat() {
    XCTAssertTrue(AudioMath.usesAacContainer("m4a"))
    XCTAssertTrue(AudioMath.usesAacContainer("aac"))
    XCTAssertFalse(AudioMath.usesAacContainer("wav"))
    XCTAssertEqual(AudioMath.effectiveFormat("m4a"), "m4a")
    XCTAssertEqual(AudioMath.effectiveFormat("aac"), "aac")
    XCTAssertEqual(AudioMath.effectiveFormat("unknown"), "wav")
  }

  func testResolveFormatDegradesUnsupported() {
    let supported = ["wav", "m4a", "aac"]
    XCTAssertEqual(AudioMath.resolveFormat("m4a", supported: supported), "m4a")
    XCTAssertEqual(AudioMath.resolveFormat("flac", supported: supported), "wav")
    XCTAssertEqual(AudioMath.resolveFormat("opus", supported: supported), "wav")
    XCTAssertEqual(AudioMath.resolveFormat(nil, supported: supported), "wav")
    XCTAssertEqual(AudioMath.resolveFormat("opus", supported: supported + ["opus"]), "opus")
  }

  func testUnknownFormatDegradesToWav() {
    XCTAssertEqual(AudioMath.effectiveFormat(nil), "wav")
    XCTAssertEqual(AudioMath.effectiveFormat("unknown"), "wav")
    XCTAssertEqual(AudioMath.effectiveFormat("m4a"), "m4a")
  }

  func testStateMachineLifecycle() throws {
    let sm = RecordingStateMachine()
    XCTAssertEqual(sm.state, RecordingStateMachine.idle)
    try sm.start()
    XCTAssertEqual(sm.state, RecordingStateMachine.recording)
    try sm.pause()
    XCTAssertEqual(sm.state, RecordingStateMachine.paused)
    try sm.resume()
    XCTAssertEqual(sm.state, RecordingStateMachine.recording)
    try sm.stop()
    XCTAssertEqual(sm.state, RecordingStateMachine.idle)
  }

  func testStateMachineRejectsInvalidTransitions() {
    let sm = RecordingStateMachine()
    XCTAssertThrowsError(try sm.stop())
    XCTAssertThrowsError(try sm.pause())
    XCTAssertThrowsError(try sm.resume())
  }
}

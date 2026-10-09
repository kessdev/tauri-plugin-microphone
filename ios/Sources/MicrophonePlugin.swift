// Copyright 2024-present

import AVFoundation
import Tauri
import UIKit
import WebKit

struct RecordingOptions: Decodable {
  var format: String?
  var sampleRate: Int?
  var channels: Int?
  var bitRate: Int?
  var maxDuration: Double?
  var deviceId: String?
  var background: Bool?
}

struct MeterOptions: Decodable {
  var intervalMs: Int?
  var scale: String?
  var floorDb: Double?
  var peakHold: Bool?
  var spectrum: Bool?
}

struct StreamOptions: Decodable {
  var sampleRate: Int?
  var channels: Int?
  var frameSamples: Int?
  var maxSeconds: Double?
}

struct AudioClip: Encodable {
  var path: String
  var duration: Double
  var format: String
}

struct Permissions: Encodable {
  var microphone: String
}

struct StreamStartArgs: Decodable {
  var options: StreamOptions?
  var channel: Channel
}

struct StreamInfo: Encodable {
  var transport: String
  var sampleRate: Int
  var channels: Int
  var frameSamples: Int
}

struct Capabilities: Encodable {
  var formats: [String]
  var preferredFormat: String
  var sampleRates: [Int]
  var channels: [Int]
  var supportsPause: Bool
  var supportsBackground: Bool
  var supportsSpectrum: Bool
  var supportsAudioStream: Bool
  var streamTransport: String
}

class MicrophonePlugin: Plugin, AVAudioRecorderDelegate {
  var webView: WKWebView!
  var recorder: AVAudioRecorder?

  let stateMachine = RecordingStateMachine()
  var outputURL: URL?
  var sampleRate = 48_000
  var channels = 1
  var currentFormat = "wav"

  var accumulatedDuration: TimeInterval = 0
  var segmentStart: Date?
  var maxDurationTimer: DispatchWorkItem?

  var meterTimer: Timer?
  var meterOptions = MeterOptions()
  var peakHoldValue = 0.0

  var audioEngine: AVAudioEngine?
  var streamChannel: Channel?
  var streamSequence: UInt64 = 0
  var streamStartDate: Date?

  override func load(webview: WKWebView) {
    self.webView = webview
  }

  private func permissionState() -> String {
    let status = AVAudioSession.sharedInstance().recordPermission
    return AudioMath.permissionState(
      granted: status == .granted,
      requestable: status == .undetermined
    )
  }

  private func currentDuration() -> Double {
    var duration = accumulatedDuration
    if let start = segmentStart {
      duration += Date().timeIntervalSince(start)
    }
    return duration
  }

  // MARK: Recording lifecycle

  @objc private func startRecording(_ invoke: Invoke) throws {
    let options = try invoke.parseArgs(RecordingOptions.self)

    if permissionState() == "denied" {
      invoke.reject("Microphone permission denied or not yet requested")
      return
    }
    do {
      try stateMachine.start()
    } catch {
      invoke.reject("\(error)")
      return
    }

    if options.background == true {
      let session = AVAudioSession.sharedInstance()
      try? session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker])
      try? session.setActive(true)
    }

    sampleRate = options.sampleRate ?? 48_000
    channels = options.channels ?? 1
    let requested = options.format
    let effective = AudioMath.effectiveFormat(requested)
    currentFormat = effective
    let url = FileManager.default.temporaryDirectory
      .appendingPathComponent(
        "tauri-microphone-\(Int(Date().timeIntervalSince1970 * 1000)).\(AudioMath.effectiveExtension(requested))"
      )
    outputURL = url

    let settings: [String: Any]
    if AudioMath.usesAacContainer(requested) {
      settings = [
        AVFormatIDKey: Int(kAudioFormatMPEG4AAC),
        AVSampleRateKey: Double(sampleRate),
        AVNumberOfChannelsKey: channels,
        AVEncoderAudioQualityKey: AVAudioQuality.high.rawValue,
        AVEncoderBitRateKey: options.bitRate ?? 128_000,
      ]
    } else {
      settings = [
        AVFormatIDKey: Int(kAudioFormatLinearPCM),
        AVSampleRateKey: Double(sampleRate),
        AVNumberOfChannelsKey: channels,
        AVLinearPCMBitDepthKey: AudioMath.bitsPerSample,
        AVLinearPCMIsFloatKey: false,
        AVLinearPCMIsBigEndianKey: false,
        AVLinearPCMIsNonInterleaved: false,
      ]
    }

    do {
      let recorder = try AVAudioRecorder(url: url, settings: settings)
      recorder.isMeteringEnabled = true
      recorder.prepareToRecord()
      recorder.record()
      self.recorder = recorder
      accumulatedDuration = 0
      segmentStart = Date()

      if let maxDuration = options.maxDuration, maxDuration > 0 {
        let work = DispatchWorkItem { [weak self] in self?.autoStop() }
        maxDurationTimer = work
        DispatchQueue.main.asyncAfter(deadline: .now() + maxDuration, execute: work)
      }
      invoke.resolve()
    } catch {
      stateMachine.forceIdle()
      invoke.reject("Failed to start recording: \(error.localizedDescription)")
    }
  }

  private func autoStop() {
    guard stateMachine.state != RecordingStateMachine.idle else { return }
    recorder?.stop()
    recorder = nil
    try? stateMachine.stop()
    segmentStart = nil
  }

  @objc private func stopRecording(_ invoke: Invoke) throws {
    guard stateMachine.state != RecordingStateMachine.idle else {
      invoke.reject("cannot stop while idle")
      return
    }
    maxDurationTimer?.cancel()
    let duration = currentDuration()
    recorder?.stop()
    recorder = nil
    segmentStart = nil
    try? stateMachine.stop()

    guard let url = outputURL else {
      invoke.reject("Failed to finalize the recording")
      return
    }
    invoke.resolve(AudioClip(path: url.path, duration: duration, format: currentFormat))
  }

  @objc private func pauseRecording(_ invoke: Invoke) {
    do {
      try stateMachine.pause()
    } catch {
      invoke.reject("\(error)")
      return
    }
    accumulatedDuration = currentDuration()
    segmentStart = nil
    recorder?.pause()
    invoke.resolve()
  }

  @objc private func resumeRecording(_ invoke: Invoke) {
    do {
      try stateMachine.resume()
    } catch {
      invoke.reject("\(error)")
      return
    }
    segmentStart = Date()
    recorder?.record()
    invoke.resolve()
  }

  @objc private func getRecordingState(_ invoke: Invoke) {
    invoke.resolve(stateMachine.state)
  }

  // MARK: Capabilities

  @objc private func getCapabilities(_ invoke: Invoke) {
    invoke.resolve(
      Capabilities(
        formats: AudioMath.supportedFormats(),
        preferredFormat: "wav",
        sampleRates: [44_100, 48_000],
        channels: [1, 2],
        supportsPause: true,
        supportsBackground: true,
        supportsSpectrum: true,
        supportsAudioStream: true,
        streamTransport: "base64"
      ))
  }

  @objc private func getSupportedFormats(_ invoke: Invoke) {
    invoke.resolve(AudioMath.supportedFormats())
  }

  // MARK: Raw audio streaming

  @objc private func startAudioStream(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(StreamStartArgs.self)
    if permissionState() == "denied" {
      invoke.reject("Microphone permission denied or not yet requested")
      return
    }
    guard audioEngine == nil else {
      invoke.reject("an audio stream is already active")
      return
    }

    let options = args.options
    let streamSampleRate = options?.sampleRate ?? 48_000
    let streamChannels = options?.channels ?? 1
    let frameSamples = options?.frameSamples ?? 1_024

    let session = AVAudioSession.sharedInstance()
    try? session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker])
    try? session.setActive(true)

    let engine = AVAudioEngine()
    let input = engine.inputNode
    let format = input.inputFormat(forBus: 0)

    input.installTap(onBus: 0, bufferSize: AVAudioFrameCount(frameSamples), format: format) {
      [weak self] buffer, _ in
      self?.emitStreamBuffer(buffer)
    }
    engine.prepare()
    do {
      try engine.start()
    } catch {
      input.removeTap(onBus: 0)
      invoke.reject("Failed to start audio stream: \(error.localizedDescription)")
      return
    }

    audioEngine = engine
    streamChannel = args.channel
    streamSequence = 0
    streamStartDate = Date()

    if let maxSeconds = options?.maxSeconds, maxSeconds > 0 {
      DispatchQueue.main.asyncAfter(deadline: .now() + maxSeconds) { [weak self] in
        self?.teardownStream()
      }
    }

    invoke.resolve(
      StreamInfo(
        transport: "base64",
        sampleRate: streamSampleRate,
        channels: streamChannels,
        frameSamples: frameSamples
      ))
  }

  @objc private func stopAudioStream(_ invoke: Invoke) {
    teardownStream()
    invoke.resolve()
  }

  private func teardownStream() {
    if let engine = audioEngine {
      engine.inputNode.removeTap(onBus: 0)
      engine.stop()
    }
    audioEngine = nil
    streamChannel = nil
    streamStartDate = nil
  }

  private func emitStreamBuffer(_ buffer: AVAudioPCMBuffer) {
    guard let channel = streamChannel else { return }
    let frameLength = Int(buffer.frameLength)
    guard frameLength > 0, let channelData = buffer.floatChannelData else { return }
    let channelCount = Int(buffer.format.channelCount)

    var samples = [Int16]()
    samples.reserveCapacity(frameLength * channelCount)
    for frame in 0..<frameLength {
      for channelIndex in 0..<channelCount {
        let value = channelData[channelIndex][frame]
        samples.append(Int16(max(-1.0, min(1.0, value)) * Float(Int16.max)))
      }
    }

    let bytes = AudioMath.int16ToLittleEndianBytes(samples)
    let timestamp = streamStartDate.map { Date().timeIntervalSince($0) } ?? 0
    let payload: JSObject = [
      "sequence": NSNumber(value: streamSequence),
      "timestamp": timestamp,
      "data": Data(bytes).base64EncodedString(),
    ]
    channel.send(payload)
    streamSequence += 1
  }

  // MARK: Metering

  @objc private func startMeter(_ invoke: Invoke) throws {
    meterOptions = try invoke.parseArgs(MeterOptions.self)
    peakHoldValue = 0.0
    let interval = Double(meterOptions.intervalMs ?? 100) / 1000.0
    DispatchQueue.main.async { [weak self] in
      self?.meterTimer?.invalidate()
      self?.meterTimer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) {
        [weak self] _ in
        self?.emitMeter()
      }
    }
    invoke.resolve()
  }

  @objc private func stopMeter(_ invoke: Invoke) {
    DispatchQueue.main.async { [weak self] in
      self?.meterTimer?.invalidate()
      self?.meterTimer = nil
    }
    invoke.resolve()
  }

  private func emitMeter() {
    guard let recorder = recorder, stateMachine.state != RecordingStateMachine.idle else { return }
    recorder.updateMeters()
    let floor = meterOptions.floorDb ?? -60.0
    let power = Double(recorder.averagePower(forChannel: 0))
    let peakPower = Double(recorder.peakPower(forChannel: 0))
    let peakLinear = AudioMath.linearLevel(fromDb: peakPower, floorDb: floor)
    let levelLinear = AudioMath.linearLevel(fromDb: power, floorDb: floor)
    let held = meterOptions.peakHold == true
      ? AudioMath.applyPeakHold(previous: peakHoldValue, current: peakLinear)
      : peakLinear
    peakHoldValue = held
    let scale = meterOptions.scale ?? "linear"
    let level = scale == "dbfs" ? Double(max(power, floor)) : levelLinear

    let data: JSObject = [
      "level": level,
      "peak": held,
      "rms": levelLinear,
      "db": Double(max(power, floor)),
      "clipping": peakPower >= 0.0,
      "timestamp": Date().timeIntervalSince1970,
    ]
    trigger("meter", data: data)
  }

  // MARK: Permissions

  @objc override func checkPermissions(_ invoke: Invoke) {
    invoke.resolve(Permissions(microphone: permissionState()))
  }

  @objc override func requestPermissions(_ invoke: Invoke) {
    AVAudioSession.sharedInstance().requestRecordPermission { granted in
      invoke.resolve(Permissions(microphone: granted ? "granted" : "denied"))
    }
  }

  @objc private func openAppSettings(_ invoke: Invoke) {
    guard let settingsUrl = URL(string: UIApplication.openSettingsURLString) else {
      invoke.reject("Application settings are not available")
      return
    }
    DispatchQueue.main.async {
      if UIApplication.shared.canOpenURL(settingsUrl) {
        UIApplication.shared.open(settingsUrl) { _ in invoke.resolve() }
      } else {
        invoke.reject("Application settings are not available")
      }
    }
  }
}

@_cdecl("init_plugin_microphone")
func initPlugin() -> Plugin {
  return MicrophonePlugin()
}

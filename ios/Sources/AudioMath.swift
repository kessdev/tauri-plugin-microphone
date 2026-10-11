import Foundation

/// Pure audio helpers, free of AVFoundation so they can be unit-tested without
/// a microphone or a device.
enum AudioMath {
  static let bitsPerSample = 16

  /// Seconds represented by `frames` at `sampleRate`.
  static func durationSeconds(frames: Int64, sampleRate: Int) -> Double {
    guard sampleRate > 0 else { return 0 }
    return Double(frames) / Double(sampleRate)
  }

  /// Converts a dBFS value (<= 0) to a linear level in 0..1.
  static func linearLevel(fromDb db: Double, floorDb: Double = -60.0) -> Double {
    if db <= floorDb { return 0 }
    let level = pow(10.0, db / 20.0)
    return min(max(level, 0.0), 1.0)
  }

  /// dBFS for a linear level, clamped to `floorDb`..0.
  static func dbfs(level: Double, floorDb: Double = -60.0) -> Double {
    if level <= 0 { return floorDb }
    let db = 20.0 * log10(level)
    return min(max(db, floorDb), 0.0)
  }

  /// Applies a peak-hold with decay: keeps the previous peak unless the current
  /// level is higher, decaying it by `decay` on each step.
  static func applyPeakHold(previous: Double, current: Double, decay: Double = 0.9) -> Double {
    return max(current, previous * decay)
  }

  /// Maps an OS permission probe to the plugin permission state.
  ///
  /// `granted` means the microphone permission is held; `requestable` means the
  /// OS can still show a prompt (permission not yet determined); otherwise it is
  /// permanently denied.
  static func permissionState(granted: Bool, requestable: Bool) -> String {
    if granted { return "granted" }
    return requestable ? "prompt" : "denied"
  }

  /// Formats guaranteed/progressive on iOS.
  static func supportedFormats() -> [String] {
    return ["wav", "m4a", "aac", "flac"]
  }

  /// Converts interleaved 16-bit samples to little-endian bytes.
  static func int16ToLittleEndianBytes(_ samples: [Int16]) -> [UInt8] {
    var bytes = [UInt8]()
    bytes.reserveCapacity(samples.count * 2)
    for sample in samples {
      let bits = UInt16(bitPattern: sample)
      bytes.append(UInt8(bits & 0xff))
      bytes.append(UInt8((bits >> 8) & 0xff))
    }
    return bytes
  }

  /// Whether the requested format is produced through the AAC/MP4 path.
  static func usesAacContainer(_ format: String?) -> Bool {
    return format == "m4a" || format == "aac"
  }

  /// File extension for a requested format, degrading unknown formats to wav.
  static func effectiveExtension(_ format: String?) -> String {
    switch format {
    case "m4a": return "m4a"
    case "aac": return "aac"
    case "flac": return "flac"
    case "opus": return "opus"
    default: return "wav"
    }
  }

  /// Resolves the format to capture: the requested one when it is supported by
  /// the platform, otherwise `wav` (never a mislabeled container).
  static func resolveFormat(_ requested: String?, supported: [String]) -> String {
    guard let requested = requested, supported.contains(requested) else { return "wav" }
    return requested
  }

  /// Resolves the capture format, degrading FLAC to `wav` when the in-process
  /// Rust encoder (`mic_flac_*`) is not linked into the current process.
  static func resolveFormat(
    _ requested: String?, supported: [String], flacEncoderAvailable: Bool
  ) -> String {
    let resolved = resolveFormat(requested, supported: supported)
    if resolved == "flac" && !flacEncoderAvailable { return "wav" }
    return resolved
  }

  static func effectiveFormat(_ format: String?) -> String {
    switch format {
    case "m4a": return "m4a"
    case "aac": return "aac"
    case "flac": return "flac"
    case "opus": return "opus"
    default: return "wav"
    }
  }
}

/// Pure recording state machine: `idle -> recording -> paused -> recording -> idle`.
final class RecordingStateMachine {
  static let idle = "idle"
  static let recording = "recording"
  static let paused = "paused"

  private(set) var state = idle

  func start() throws {
    guard state == RecordingStateMachine.idle else {
      throw StateError.invalid("cannot start while \(state)")
    }
    state = RecordingStateMachine.recording
  }

  func stop() throws {
    guard state != RecordingStateMachine.idle else {
      throw StateError.invalid("cannot stop while idle")
    }
    state = RecordingStateMachine.idle
  }

  func pause() throws {
    guard state == RecordingStateMachine.recording else {
      throw StateError.invalid("cannot pause while \(state)")
    }
    state = RecordingStateMachine.paused
  }

  func resume() throws {
    guard state == RecordingStateMachine.paused else {
      throw StateError.invalid("cannot resume while \(state)")
    }
    state = RecordingStateMachine.recording
  }

  func forceIdle() {
    state = RecordingStateMachine.idle
  }
}

enum StateError: Error, Equatable {
  case invalid(String)
}

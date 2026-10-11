// Opus encoding for iOS uses the libopus C library exposed as a `COpus` system
// module. libopus must be packaged as an xcframework for device and simulator
// (see task 7.2). Until that module is present in the SwiftPM manifest, this
// file compiles to nothing and the plugin does not advertise Opus on iOS.
#if canImport(COpus)
  import COpus
  import Foundation

  enum OpusSupport {
    static let sampleRate: Int32 = 48_000
    static let frameSize: Int = 960  // 20 ms at 48 kHz

    final class Encoder {
      private var state: OpaquePointer?
      private let channels: Int32

      init?(channels: Int) {
        self.channels = Int32(channels)
        var error: Int32 = 0
        let application = Int32(OPUS_APPLICATION_AUDIO.rawValue)
        self.state = opus_encoder_create(sampleRate, self.channels, application, &error)
        if self.state == nil || error != OPUS_OK {
          return nil
        }
      }

      deinit {
        if let state {
          opus_encoder_destroy(state)
        }
      }

      func encode(_ pcm: [Int16], frameSize: Int) -> Data? {
        guard let state else { return nil }
        var output = [UInt8](repeating: 0, count: 4_000)
        let encoded = pcm.withUnsafeBufferPointer { input in
          output.withUnsafeMutableBufferPointer { out in
            opus_encode(
              state,
              input.baseAddress,
              Int32(frameSize),
              out.baseAddress,
              Int32(output.count)
            )
          }
        }
        guard encoded > 0 else { return nil }
        return Data(output.prefix(Int(encoded)))
      }
    }
  }
#endif

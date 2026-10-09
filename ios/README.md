# tauri-plugin-microphone-kessdev (iOS)

Swift implementation of the microphone plugin for iOS, using AVFoundation.

## Requirements

Add the following to the app's `Info.plist`:

- `NSMicrophoneUsageDescription` — required to access the microphone.
- `UIBackgroundModes` containing `audio` — required for background recording.

## Opus (optional)

iOS Opus recording uses the libopus C library through a `COpus` SwiftPM system
module, which must be packaged as an `xcframework` for device and simulator
(task 7.2). Until that module is linked, `OpusSupport.swift` compiles to nothing
and the plugin does not advertise Opus on iOS (`getSupportedFormats()` omits it).

To enable it:

1. Build/obtain `libopus` and create `libopus.xcframework` (device + simulator).
2. Add a `COpus` system library target/module map to `Package.swift` pointing at
   the headers and linking the static library.
3. Re-run the Swift tests and add `"opus"` to the iOS format list.

# tauri-plugin-microphone-kessdev (iOS)

Swift implementation of the microphone plugin for iOS, using AVFoundation.

## Requirements

Add the following to the app's `Info.plist`:

- `NSMicrophoneUsageDescription` — required to access the microphone.
- `UIBackgroundModes` containing `audio` — required for background recording.

## FLAC

FLAC recording captures PCM with `AVAudioEngine` and encodes it in-process with
the pure-Rust `flacenc` encoder exported by the app's Rust library
(`mic_flac_open`/`mic_flac_write`/`mic_flac_finish`). Those symbols are resolved
at runtime with `dlsym`, so the Swift package still builds and tests on its own;
when they are not linked (for example the standalone package build), a FLAC
request degrades to WAV and reports `format: 'wav'` instead of an invalid file.

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

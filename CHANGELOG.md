# Changelog

## [0.1.1-beta.0]

- [`611a376`](https://github.com/kessdev/tauri-plugin-microphone/commit/611a3767c596133bdb6733d245d84b72d000b879) Fix the CI verification across platforms: use npm as the single package manager
    in the example app, install `libasound2-dev`/`pkg-config` for the Linux desktop
    build, and resolve the in-process FLAC encoder at runtime on iOS so the Swift
    package links without the Rust static library. Also make desktop
    `onMeter`/`getAnalyserNode()` work regardless of subscription order and document
    that native-format desktop recording has no metering.
- [`21ec782`](https://github.com/kessdev/tauri-plugin-microphone/commit/21ec7824d5372b0dddb210eec66bb7336ac58a45) Add FLAC recording on Android and iOS (in-process `flacenc` encoder, Option B)
    and make unsupported-format degradation deterministic so the reported format,
    file extension and container always match.

## 0.1.0-beta.0

- Initial beta: cross-platform microphone recording, live metering, raw audio
  streaming, permissions and background recording scaffolding.

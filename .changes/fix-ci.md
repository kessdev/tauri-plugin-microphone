---
"tauri-plugin-microphone-kessdev": "patch"
"tauri-plugin-microphone-kessdev-js": "patch"
---

Fix the CI verification across platforms: use npm as the single package manager
in the example app, install `libasound2-dev`/`pkg-config` for the Linux desktop
build, and resolve the in-process FLAC encoder at runtime on iOS so the Swift
package links without the Rust static library. Also make desktop
`onMeter`/`getAnalyserNode()` work regardless of subscription order and document
that native-format desktop recording has no metering.

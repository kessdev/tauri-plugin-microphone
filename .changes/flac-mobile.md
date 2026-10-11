---
"tauri-plugin-microphone-kessdev": "patch"
"tauri-plugin-microphone-kessdev-js": "patch"
---

Add FLAC recording on Android and iOS (in-process `flacenc` encoder, Option B)
and make unsupported-format degradation deterministic so the reported format,
file extension and container always match.

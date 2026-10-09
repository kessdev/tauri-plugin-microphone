# tauri-plugin-microphone-kessdev

Microphone plugin for **Tauri v2**: cross-platform audio recording, live
metering and raw PCM streaming on **Android, iOS, macOS, Windows and Linux**
behind a single JavaScript API.

> **Development note (AI assistance).** This plugin is being developed with the
> assistance of AI tools. AI may have generated parts of the code, tests and
> documentation. Every change is reviewed and verified by maintainers (automated
> tests and CI) before release, but we disclose this for transparency. Please
> report any issue you find.

## Install

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-microphone-kessdev = "0.1"
```

```bash
npm install tauri-plugin-microphone-kessdev
```

```rust
// src-tauri/src/lib.rs
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_microphone_kessdev::init())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
```

### Prerequisites

- **Desktop**: the guaranteed native path builds `cpal` (Linux needs
  `libasound2-dev`; the other platforms use their system audio backend) and
  `opus-head-sys`, which compiles the vendored libopus with **CMake** (a C
  toolchain is required). FLAC uses the pure-Rust `flacenc`, no system library.
- **Android**: declare `android.permission.RECORD_AUDIO` (the plugin manifest
  already does).
- **iOS/macOS**: add `NSMicrophoneUsageDescription` to `Info.plist`.

## Usage

```ts
import {
  startRecording,
  stopRecording,
  pauseRecording,
  resumeRecording,
  getRecordingState,
  onMeter,
  startAudioStream,
  stopAudioStream,
  checkPermissions,
  requestPermissions,
  openAppSettings,
  getCapabilities
} from 'tauri-plugin-microphone-kessdev'

await requestPermissions()
await startRecording({ format: 'wav' })
// mobile & desktop native formats -> { duration, format, path }
// desktop webview formats -> { duration, format, blob, url }
const clip = await stopRecording()
```

## API

| Function | Description |
|----------|-------------|
| `startRecording(options?)` | Start recording. |
| `stopRecording()` | Stop and return the `AudioClip`. |
| `pauseRecording()` | Pause without including the paused interval. |
| `resumeRecording()` | Resume a paused recording. |
| `getRecordingState()` | `idle` \| `recording` \| `paused`. |
| `onMeter(cb, options?) => UnlistenFn` | Live level updates. |
| `getAnalyserNode()` | Desktop: the `AnalyserNode` for custom visualizers. |
| `startAudioStream(options, onChunk)` | Raw PCM frames over a `Channel`. |
| `stopAudioStream()` | Stop the stream. |
| `checkPermissions()` | OS microphone permission state. |
| `requestPermissions()` | Request OS microphone permission. |
| `openAppSettings()` | Open app settings (mobile). |
| `getCapabilities()` | Supported formats, rates, channels and flags. |
| `getSupportedFormats()` | List of supported output formats. |

The Tauri commands backing these functions are declared in `build.rs` and
enumerated in `permissions/default.toml`.

### Recording options

```ts
interface RecordingOptions {
  format?: 'wav' | 'm4a' | 'aac' | 'flac' | 'opus' // preference; real format is reported
  sampleRate?: number   // Hz (default 48000)
  channels?: number     // 1 or 2 (default 1)
  bitRate?: number      // bits/s, for compressed formats
  maxDuration?: number  // seconds; automatically stops the recording
  deviceId?: string     // desktop input device name
  background?: boolean  // mobile only; desktop rejects with an error
}
```

Unknown `deviceId` on desktop rejects with a descriptive error. `checkPermissions`
and `requestPermissions` resolve to `'granted' | 'denied' | 'prompt'`.

## Metering

```ts
const unlisten = onMeter((data) => console.log(data.level), {
  intervalMs: 100,   // update period in ms
  scale: 'linear',   // 'linear' (0..1) or 'dbfs' (<= 0)
  floorDb: -60,      // dB floor when scale is 'dbfs'
  peakHold: true,    // hold the peak with decay
  spectrum: true     // include frequency bands (where supported)
})
unlisten()
```

`MeterData`:

```ts
interface MeterData {
  level: number        // 0..1 (linear) or dBFS (<= 0)
  peak?: number        // 0..1
  rms?: number         // 0..1
  db?: number          // dBFS
  clipping?: boolean
  spectrum?: number[]  // normalized band magnitudes (where supported)
  timestamp: number    // seconds
}
```

Recommended `intervalMs`: **100 ms** for meters and VU displays, **33–50 ms**
(~20–30 fps) for smooth visualizers. Going below ~16 ms adds work without
visible benefit, since updates are bounded by the display refresh rate.

On **desktop** metering uses a Web Audio `AnalyserNode` (zero IPC). Use
`getAnalyserNode()` to obtain it for custom visualization; it returns `null` on
mobile, where the native backend computes and delivers `MeterData` instead.

## Return contract

```ts
interface AudioClip {
  duration: number                  // seconds
  format: 'wav' | 'm4a' | 'aac' | 'flac' | 'opus' | string  // real format
  path?: string                     // mobile, and desktop native path
  blob?: Blob                       // desktop
  url?: string                      // desktop object URL (revoke after use)
}
```

- **Mobile**: the native backend writes a temporary file and returns `path`.
- **Desktop**: requesting a non-native format uses the webview `MediaRecorder`
  and returns `blob` + `url`. Requesting a native format (`wav`, `flac`, `opus`)
  returns `path`; the facade also materializes `blob` + `url` when the Tauri
  **asset protocol** is enabled (`app.security.assetProtocol.enable = true` and a
  matching scope), and otherwise returns `path` only.
- `format` is a **preference**: `AudioClip.format` always reports the format
  actually produced.

## Raw audio streaming

```ts
const stream = await startAudioStream({ sampleRate: 48000, frameSamples: 1024 },
  (chunk) => {
    // chunk.data: ArrayBuffer of interleaved 16-bit PCM
    // chunk.sequence, chunk.timestamp, chunk.droppedBefore?
    myWasmVad.process(chunk.data)
  })
// ...
await stream.stop()
```

- The stream always carries **raw PCM** (16-bit, interleaved), independent of the
  file format used for `startRecording`.
- **Desktop** uses transport `raw`: each `Channel` frame is a binary
  `[sequence: u64 LE][timestamp: f64 LE][pcm: i16 LE ...]` in a single
  `ArrayBuffer`. Inter-frame gaps in `sequence` are surfaced as `droppedBefore`.
- **Mobile** uses transport `base64`: each `Channel` message is
  `{ sequence, timestamp, data: base64(pcm) }`, normalized by the facade.
- `info.transport` (`'raw'` | `'base64'`) tells you which transport is in use.
- **Backpressure**: the desktop capture keeps a bounded queue (64 frames) and
  drops the oldest frame when the consumer lags, reporting the drop as a
  `sequence` gap. Pass `maxSeconds` to auto-stop after a duration.
- Commands `start_audio_stream` / `stop_audio_stream` are gated by the
  `allow-start-audio-stream` / `allow-stop-audio-stream` permissions (included in
  the default set).

## Capabilities

```ts
const caps = await getCapabilities()
// {
//   formats: ['wav', ...],      // output formats available on this platform
//   preferredFormat: 'wav',     // best default choice
//   sampleRates: [44100, 48000],
//   channels: [1, 2],
//   supportsPause: true,
//   supportsBackground: false,
//   supportsSpectrum: true,
//   supportsAudioStream: true,
//   streamTransport: 'raw'      // 'raw' (desktop) | 'base64' (mobile)
// }
```

Use capabilities to pick a format instead of hardcoding:

1. If the caller has no explicit preference, request `caps.preferredFormat`
   (always `wav` today, guaranteed everywhere).
2. Otherwise, if `caps.formats` contains the requested format, request it; the
   effective format is confirmed in `AudioClip.format`.
3. If it is not listed, request `wav` (or accept the documented degradation) to
   avoid an invalid file.

The `formats` list mirrors the `AudioFormat` union in TypeScript and the Rust
`AudioFormat` enum, and is reflected by `getSupportedFormats()` and the tests.

## Format matrix

| Format | Android | iOS | Windows | Linux | macOS |
|--------|---------|-----|---------|-------|-------|
| WAV/PCM | AudioRecord + WAV header | AVAudioRecorder LPCM | cpal + hound | cpal + hound | cpal + hound |
| FLAC | — | — | `flacenc` | `flacenc` | `flacenc` |
| Opus | MediaRecorder OGG (API 29+) | pending (libopus FFI) | vendored libopus | vendored libopus | vendored libopus |
| M4A/AAC | MediaRecorder | AVAudioRecorder (AAC) | webview `MediaRecorder` | webview `MediaRecorder` | webview `MediaRecorder` |

WAV/PCM is guaranteed on every platform. Other formats are progressive; a `—`
means the platform does not produce that format today, and requesting it degrades
to WAV and reports the effective format instead of returning an invalid file.
`getSupportedFormats()` / `getCapabilities()` are the source of truth for the
current platform.

### M4A/AAC notes

- **Android** records M4A/AAC natively (`MediaRecorder`, MPEG-4 + AAC).
- **iOS** records M4A/AAC through the system codec (`AVAudioRecorder`, AAC).
- **Windows/Linux/macOS desktop**: the webview `MediaRecorder` may produce
  `audio/mp4` (AAC); when it does, the clip reports `format: 'aac'`. AAC encoders
  on Windows/Linux carry their own licensing (AAC is patent-encumbered; e.g.
  `fdk-aac` has its own terms), which is **not** covered by this project, so the
  guaranteed native desktop path does not produce AAC. Check
  `getCapabilities().formats` before requesting M4A/AAC on desktop.

### FLAC notes

FLAC is **lossless compressed** audio: it preserves the exact PCM samples of WAV
at roughly half the size. It is produced today on **desktop** (Windows, Linux and
macOS) by the pure-Rust `flacenc` encoder (no C dependency). **Android and iOS do
not support FLAC yet**, so `getSupportedFormats()` omits it on those platforms;
check it before requesting FLAC. Because it is lossless, FLAC is the preferred
archival alternative to WAV when size matters.

### Opus notes

- **Android** encodes Ogg/Opus natively with `MediaRecorder` (OGG container +
  `OPUS` encoder), available on **API 29+**; on older APIs an Opus request
  degrades to WAV and reports `format: 'wav'`.
- **Desktop** encodes Ogg/Opus with the vendored **libopus** C library
  (`opus-head-sys`, compiled via CMake) and writes a valid Ogg/Opus container.
  This adds a C build step (needs a C toolchain and CMake) but no system libopus
  install.
- **iOS** Opus is implemented but **pending**: it requires packaging `libopus` as
  an xcframework and linking a `COpus` SwiftPM module. Until that is done the
  code is inactive, iOS does not advertise Opus and `getSupportedFormats()` omits
  it (see the `add-ios-opus-tests` change).

Opus is lossy and low-latency (ideal for voice/streaming); use WAV/FLAC when
lossless fidelity is required.

## Permissions

Two independent layers:

1. **OS permission** — `checkPermissions()`, `requestPermissions()`,
   `openAppSettings()`. On desktop, requesting is `getUserMedia({ audio: true })`
   and `openAppSettings()` rejects with a descriptive error.
2. **Tauri capabilities** — `permissions/default.toml` lists the `allow-*`
   permissions for every command. Plugin events (used by `onMeter` on mobile)
   are gated through the `register_listener` / `remove_listener` commands, so the
   default set includes `allow-register-listener` and `allow-remove-listener`.
   Add the default set to your capability:

```json
{
  "permissions": ["microphone-kessdev:default"]
}
```

The OS permission and the Tauri capability are independent: granting the
capability lets the webview call the commands, but the OS may still deny access
to the microphone (and `checkPermissions()`/`requestPermissions()` report that).
The listener capability is required for `onMeter` to receive `meter` events.

Platform configuration:

- **Android**: declare `android.permission.RECORD_AUDIO` (the plugin manifest
  already does).
- **iOS/macOS**: add `NSMicrophoneUsageDescription` to `Info.plist`.

## Background recording

Pass `background: true` to `startRecording` to keep recording while the app is in
the background.

- **Android** starts a foreground service of type `microphone`. The plugin
  manifest declares:
  - `android.permission.RECORD_AUDIO`
  - `android.permission.FOREGROUND_SERVICE`
  - `android.permission.FOREGROUND_SERVICE_MICROPHONE`
  - `android.permission.POST_NOTIFICATIONS` (Android 13+; needed to show the
    persistent notification, request it at runtime).
- **iOS** requires `UIBackgroundModes` = `audio` in `Info.plist`; the plugin sets
  the `AVAudioSession` category to `playAndRecord` and calls `setActive(true)`.
- **Desktop** does not support background recording:
  `getCapabilities().supportsBackground === false` and requesting
  `background: true` rejects with a descriptive error.

`Info.plist` keys used by the plugin:

- `NSMicrophoneUsageDescription` — microphone access.
- `UIBackgroundModes` (containing `audio`) — background recording.

## Development

```bash
# JavaScript
npm install && npm test && npm run build

# Rust (desktop)
cargo test && cargo clippy --all-targets -- -D warnings

# Example app (desktop)
cd examples/tauri-app && npx tauri dev

# Native suites (need Android SDK/NDK or Xcode)
cd examples/tauri-app && npx tauri android init && npx tauri android build --debug
cd examples/tauri-app/src-tauri/gen/android && ./gradlew test   # Kotlin
cd ios && xcodebuild test -scheme tauri-plugin-microphone        # Swift
```

## License

MIT

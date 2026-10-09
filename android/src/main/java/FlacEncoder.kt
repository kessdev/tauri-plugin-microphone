package app.tauri.microphone

/**
 * Thin JNI bridge to the in-process Rust FLAC encoder (`mic_flac_*`).
 *
 * The native symbols are exported by the app's Rust library; the capture thread
 * calls [nativeWrite] for every PCM buffer and [nativeFinish] when recording
 * stops. A handle of `0` means the encoder could not be created.
 */
internal object FlacEncoder {
  external fun nativeOpen(sampleRate: Int, channels: Int): Long
  external fun nativeWrite(handle: Long, pcm: ShortArray, samples: Int): Int
  external fun nativeFinish(handle: Long, path: String): Int
}

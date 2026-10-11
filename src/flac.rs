use std::path::Path;

use crate::error::{Error, Result};

/// Encodes interleaved 16-bit PCM samples to a FLAC file (pure Rust).
///
/// Shared by the desktop native path and, on mobile, the in-process encoder
/// exposed through the C-ABI functions below.
pub fn encode_flac(path: &Path, samples: &[i16], channels: u16, sample_rate: u32) -> Result<()> {
  use flacenc::component::BitRepr;
  use flacenc::error::Verify;

  let signal: Vec<i32> = samples.iter().map(|&s| i32::from(s)).collect();
  let config = flacenc::config::Encoder::default()
    .into_verified()
    .map_err(|(_, error)| Error::message(format!("FLAC config error: {error:?}")))?;
  let source =
    flacenc::source::MemSource::from_samples(&signal, channels as usize, 16, sample_rate as usize);
  let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
    .map_err(|error| Error::message(format!("FLAC encoding failed: {error}")))?;
  let mut sink = flacenc::bitsink::ByteSink::new();
  stream
    .write(&mut sink)
    .map_err(|error| Error::message(format!("FLAC serialization failed: {error:?}")))?;
  std::fs::write(path, sink.as_slice()).map_err(Error::Io)
}

// ---------------------------------------------------------------------------
// In-process encoder exposed to the native backends (Option B).
//
// The native capture hands PCM to these C-ABI functions and the encoder keeps
// it buffered in memory, writing the `.flac` when `mic_flac_finish` is called.
// Android calls them through the JNI wrappers below; iOS declares them in Swift
// with `@_silgen_name`. No `panic` crosses the FFI boundary.
// ---------------------------------------------------------------------------

/// Opaque in-process FLAC encoder handle.
pub struct FlacHandle {
  sample_rate: u32,
  channels: u16,
  pcm: Vec<i16>,
}

/// Creates an encoder for `sample_rate` / `channels`; returns null on error.
///
/// # Safety
/// Callers must pass a valid `sample_rate` and `channels` (1 or 2).
#[allow(dead_code)]
#[no_mangle]
pub unsafe extern "C" fn mic_flac_open(
  sample_rate: std::os::raw::c_int,
  channels: std::os::raw::c_int,
) -> *mut FlacHandle {
  std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    if sample_rate <= 0 || channels <= 0 || channels > 2 {
      return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(FlacHandle {
      sample_rate: sample_rate as u32,
      channels: channels as u16,
      pcm: Vec::new(),
    }))
  }))
  .unwrap_or(std::ptr::null_mut())
}

/// Appends `samples` interleaved 16-bit PCM samples. Returns 0 on success, <0 on error.
///
/// # Safety
/// `handle` must come from [`mic_flac_open`] and `pcm` must point to at least
/// `samples` interleaved values (frames * channels).
#[allow(dead_code)]
#[no_mangle]
pub unsafe extern "C" fn mic_flac_write(
  handle: *mut FlacHandle,
  pcm: *const i16,
  samples: std::os::raw::c_int,
) -> std::os::raw::c_int {
  std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    if handle.is_null() || pcm.is_null() || samples <= 0 {
      return -1;
    }
    let handle = &mut *handle;
    let slice = std::slice::from_raw_parts(pcm, samples as usize);
    handle.pcm.extend_from_slice(slice);
    0
  }))
  .unwrap_or(-1)
}

/// Finalizes the `.flac` file at `path_utf8` and consumes the handle.
/// Returns 0 on success, <0 on error.
///
/// # Safety
/// `handle` must come from [`mic_flac_open`] and not be used afterwards;
/// `path_utf8`/`path_len` must describe a valid UTF-8 path.
#[allow(dead_code)]
#[no_mangle]
pub unsafe extern "C" fn mic_flac_finish(
  handle: *mut FlacHandle,
  path_utf8: *const std::os::raw::c_char,
  path_len: std::os::raw::c_int,
) -> std::os::raw::c_int {
  std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    if handle.is_null() || path_utf8.is_null() || path_len <= 0 {
      return -1;
    }
    let handle = Box::from_raw(handle);
    let bytes = std::slice::from_raw_parts(path_utf8 as *const u8, path_len as usize);
    let path = match std::str::from_utf8(bytes) {
      Ok(value) => std::path::PathBuf::from(value),
      Err(_) => return -2,
    };
    match encode_flac(&path, &handle.pcm, handle.channels, handle.sample_rate) {
      Ok(()) => 0,
      Err(_) => -3,
    }
  }))
  .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::{SystemTime, UNIX_EPOCH};

  fn unique_path(extension: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_nanos())
      .unwrap_or(0);
    std::env::temp_dir().join(format!("tauri-microphone-flac-{nanos}.{extension}"))
  }

  #[test]
  fn encodes_a_valid_flac_file() {
    let path = unique_path("flac");
    let samples: Vec<i16> = (0..4096).map(|i| ((i % 100) as i16) * 50).collect();
    encode_flac(&path, &samples, 1, 48_000).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"fLaC"));
    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn ffi_api_encodes_a_valid_flac_file() {
    let path = unique_path("flac");
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let pcm: Vec<i16> = (0..9_600).map(|i| ((i % 100) as i16) * 50).collect();
    unsafe {
      let handle = mic_flac_open(48_000, 1);
      assert!(!handle.is_null());
      assert_eq!(mic_flac_write(handle, pcm.as_ptr(), 9_600), 0);
      assert_eq!(
        mic_flac_finish(handle, c_path.as_ptr(), c_path.as_bytes().len() as i32),
        0
      );
    }
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"fLaC"));
    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn ffi_api_rejects_invalid_arguments() {
    unsafe {
      assert!(mic_flac_open(48_000, 3).is_null());
      assert!(mic_flac_open(0, 1).is_null());
      assert_eq!(
        mic_flac_write(std::ptr::null_mut(), std::ptr::null(), 10),
        -1
      );
    }
  }
}

/// JNI wrappers for the Kotlin `app.tauri.microphone.FlacEncoder` object.
#[cfg(target_os = "android")]
mod android_jni {
  use super::{mic_flac_finish, mic_flac_open, mic_flac_write, FlacHandle};
  use jni::objects::{JClass, JShortArray, JString};
  use jni::sys::{jint, jlong, jshortArray};
  use jni::JNIEnv;

  #[no_mangle]
  pub extern "system" fn Java_app_tauri_microphone_FlacEncoder_nativeOpen(
    _env: JNIEnv,
    _class: JClass,
    sample_rate: jint,
    channels: jint,
  ) -> jlong {
    unsafe { mic_flac_open(sample_rate, channels) as jlong }
  }

  #[no_mangle]
  pub extern "system" fn Java_app_tauri_microphone_FlacEncoder_nativeWrite(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pcm: jshortArray,
    samples: jint,
  ) -> jint {
    if handle == 0 || samples <= 0 {
      return -1;
    }
    let array = unsafe { JShortArray::from_raw(pcm) };
    let len = match env.get_array_length(&array) {
      Ok(len) => len,
      Err(_) => return -1,
    };
    let take = samples.min(len) as usize;
    let mut buffer = vec![0_i16; len as usize];
    if env.get_short_array_region(&array, 0, &mut buffer).is_err() {
      return -1;
    }
    buffer.truncate(take);
    unsafe { mic_flac_write(handle as *mut FlacHandle, buffer.as_ptr(), take as jint) }
  }

  #[no_mangle]
  pub extern "system" fn Java_app_tauri_microphone_FlacEncoder_nativeFinish(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    path: JString,
  ) -> jint {
    if handle == 0 {
      return -1;
    }
    let path: String = match env.get_string(&path) {
      Ok(value) => value.into(),
      Err(_) => return -1,
    };
    let bytes = path.as_bytes();
    unsafe {
      mic_flac_finish(
        handle as *mut FlacHandle,
        bytes.as_ptr() as *const std::os::raw::c_char,
        bytes.len() as jint,
      )
    }
  }
}

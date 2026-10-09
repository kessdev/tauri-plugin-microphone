use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc::sync_channel, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::de::DeserializeOwned;
use tauri::{
  ipc::{Channel, InvokeResponseBody},
  plugin::PluginApi,
  AppHandle, Runtime,
};

use crate::error::{Error, Result};
use crate::models::*;
use crate::state::RecordingStateMachine;

pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  _api: PluginApi<R, C>,
) -> crate::Result<Microphone<R>> {
  Ok(Microphone {
    app: app.clone(),
    state: Mutex::new(RecordingStateMachine::new()),
    recorder: Mutex::new(None),
    stream: Mutex::new(None),
  })
}

/// Access to the microphone APIs on desktop.
///
/// Desktop uses the webview (`getUserMedia`/`MediaRecorder`/Web Audio) through
/// the JavaScript facade, plus this guaranteed native capture path (cpal +
/// hound) so WAV/PCM output is identical across platforms. This type owns the
/// shared recording state machine and the active native recorder.
pub struct Microphone<R: Runtime> {
  #[allow(dead_code)]
  app: AppHandle<R>,
  state: Mutex<RecordingStateMachine>,
  recorder: Mutex<Option<ActiveRecorder>>,
  stream: Mutex<Option<ActiveStream>>,
}

struct ActiveStream {
  stop: Arc<AtomicBool>,
  thread: Option<JoinHandle<()>>,
}

/// Bounded frame queue shared between the audio callback and the sender thread.
struct StreamBuffer {
  pending: Vec<i16>,
  queue: VecDeque<Vec<u8>>,
  capacity: usize,
  /// Frames dropped due to backpressure. On the raw desktop transport the
  /// drop is also observable as a gap in `sequence`.
  #[allow(dead_code)]
  dropped: u64,
  sequence: u64,
}

/// Where captured PCM is written: a WAV file directly, or an in-memory buffer
/// that is encoded to FLAC or Ogg/Opus on stop.
enum Sink {
  Wav(hound::WavWriter<std::io::BufWriter<std::fs::File>>),
  Pcm(Vec<i16>),
  Opus(OpusSink),
}

/// Streaming Ogg/Opus encoder backed by the vendored libopus (`opus-head-sys`).
///
/// The raw encoder pointer is only ever touched while holding the enclosing
/// `Mutex`, so it is safe to treat the sink as `Send`.
struct OpusSink {
  encoder: *mut opus_head_sys::OpusEncoder,
  channels: u16,
  sample_rate: u32,
  frame_size: usize,
  pending: Vec<i16>,
  packets: Vec<Vec<u8>>,
  total_frames: u64,
}

unsafe impl Send for OpusSink {}

impl OpusSink {
  fn new(channels: u16, sample_rate: u32) -> Result<Self> {
    let mut error: std::os::raw::c_int = 0;
    let encoder = unsafe {
      opus_head_sys::opus_encoder_create(
        sample_rate as opus_head_sys::opus_int32,
        channels as std::os::raw::c_int,
        opus_head_sys::OPUS_APPLICATION_AUDIO as std::os::raw::c_int,
        &mut error,
      )
    };
    if encoder.is_null() || error != opus_head_sys::OPUS_OK as std::os::raw::c_int {
      return Err(Error::message(format!(
        "failed to create Opus encoder (error {error})"
      )));
    }
    Ok(Self {
      encoder,
      channels,
      sample_rate,
      // 20 ms frames.
      frame_size: (sample_rate / 50).max(1) as usize,
      pending: Vec::new(),
      packets: Vec::new(),
      total_frames: 0,
    })
  }

  fn push(&mut self, samples: &[i16]) -> Result<()> {
    self.pending.extend_from_slice(samples);
    let frame_len = self.frame_size * self.channels.max(1) as usize;
    while self.pending.len() >= frame_len {
      let frame: Vec<i16> = self.pending.drain(0..frame_len).collect();
      let mut out = vec![0_u8; 4000];
      let encoded = unsafe {
        opus_head_sys::opus_encode(
          self.encoder,
          frame.as_ptr(),
          self.frame_size as std::os::raw::c_int,
          out.as_mut_ptr(),
          out.len() as opus_head_sys::opus_int32,
        )
      };
      if encoded < 0 {
        return Err(Error::message(format!("Opus encoding failed ({encoded})")));
      }
      out.truncate(encoded as usize);
      self.packets.push(out);
      self.total_frames += self.frame_size as u64;
    }
    Ok(())
  }

  fn finish(self, path: &Path) -> Result<()> {
    write_ogg_opus(
      path,
      self.channels,
      self.sample_rate,
      self.frame_size,
      &self.packets,
    )
  }
}

impl Drop for OpusSink {
  fn drop(&mut self) {
    if !self.encoder.is_null() {
      unsafe { opus_head_sys::opus_encoder_destroy(self.encoder) };
      self.encoder = std::ptr::null_mut();
    }
  }
}

/// Writes an Ogg/Opus file (`OpusHead`, `OpusTags`, then audio packets).
fn write_ogg_opus(
  path: &Path,
  channels: u16,
  sample_rate: u32,
  frame_size: usize,
  packets: &[Vec<u8>],
) -> Result<()> {
  use ogg::{PacketWriteEndInfo, PacketWriter};

  let file = std::fs::File::create(path).map_err(Error::Io)?;
  let mut writer = PacketWriter::new(std::io::BufWriter::new(file));
  let serial = 0x7461_7572_u32;
  const PRE_SKIP: u64 = 312;

  let mut head = Vec::with_capacity(19);
  head.extend_from_slice(b"OpusHead");
  head.push(1);
  head.push(channels as u8);
  head.extend_from_slice(&(PRE_SKIP as u16).to_le_bytes());
  head.extend_from_slice(&sample_rate.to_le_bytes());
  head.extend_from_slice(&0_i16.to_le_bytes());
  head.push(0);
  writer
    .write_packet(head, serial, PacketWriteEndInfo::EndPage, 0)
    .map_err(|e| Error::message(format!("Ogg write failed: {e}")))?;

  let vendor = b"tauri-plugin-microphone";
  let mut tags = Vec::new();
  tags.extend_from_slice(b"OpusTags");
  tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
  tags.extend_from_slice(vendor);
  tags.extend_from_slice(&0_u32.to_le_bytes());
  let tags_end = if packets.is_empty() {
    PacketWriteEndInfo::EndStream
  } else {
    PacketWriteEndInfo::EndPage
  };
  writer
    .write_packet(tags, serial, tags_end, 0)
    .map_err(|e| Error::message(format!("Ogg write failed: {e}")))?;

  let mut granule = PRE_SKIP;
  for (index, packet) in packets.iter().enumerate() {
    granule += frame_size as u64;
    let end = if index + 1 == packets.len() {
      PacketWriteEndInfo::EndStream
    } else {
      PacketWriteEndInfo::EndPage
    };
    writer
      .write_packet(packet.clone(), serial, end, granule)
      .map_err(|e| Error::message(format!("Ogg write failed: {e}")))?;
  }

  use std::io::Write;
  writer.into_inner().flush().map_err(Error::Io)
}

struct ActiveRecorder {
  stop: Arc<AtomicBool>,
  paused: Arc<AtomicBool>,
  frames: Arc<AtomicU64>,
  thread: Option<JoinHandle<()>>,
  path: PathBuf,
  sample_rate: u32,
  format: AudioFormat,
}

impl<R: Runtime> Microphone<R> {
  fn with_state<T>(&self, f: impl FnOnce(&mut RecordingStateMachine) -> Result<T>) -> Result<T> {
    let mut guard = self
      .state
      .lock()
      .map_err(|_| Error::message("state lock poisoned"))?;
    f(&mut guard)
  }

  pub fn get_recording_state(&self) -> Result<RecordingState> {
    self.with_state(|sm| Ok(sm.state()))
  }

  pub fn start_recording(&self, options: &RecordingOptions) -> Result<()> {
    ensure_background_supported(options.background, false)?;
    self.with_state(|sm| sm.start())?;
    match self.spawn_recorder(options) {
      Ok(recorder) => {
        *self
          .recorder
          .lock()
          .map_err(|_| Error::message("recorder lock poisoned"))? = Some(recorder);
        Ok(())
      }
      Err(err) => {
        self.with_state(|sm| {
          sm.force_idle();
          Ok(())
        })?;
        Err(err)
      }
    }
  }

  pub fn stop_recording(&self) -> Result<AudioClip> {
    self.with_state(|sm| sm.stop())?;
    let recorder = self
      .recorder
      .lock()
      .map_err(|_| Error::message("recorder lock poisoned"))?
      .take()
      .ok_or_else(|| Error::InvalidState("no active recorder".into()))?;

    recorder.stop.store(true, Ordering::Relaxed);
    if let Some(thread) = recorder.thread {
      let _ = thread.join();
    }
    let duration = if recorder.sample_rate > 0 {
      recorder.frames.load(Ordering::Relaxed) as f64 / recorder.sample_rate as f64
    } else {
      0.0
    };
    Ok(AudioClip {
      duration,
      format: recorder.format.as_str().into(),
      path: Some(recorder.path.to_string_lossy().into_owned()),
    })
  }

  pub fn pause_recording(&self) -> Result<()> {
    self.with_state(|sm| sm.pause())?;
    if let Some(recorder) = self
      .recorder
      .lock()
      .map_err(|_| Error::message("recorder lock poisoned"))?
      .as_ref()
    {
      recorder.paused.store(true, Ordering::Relaxed);
    }
    Ok(())
  }

  pub fn resume_recording(&self) -> Result<()> {
    self.with_state(|sm| sm.resume())?;
    if let Some(recorder) = self
      .recorder
      .lock()
      .map_err(|_| Error::message("recorder lock poisoned"))?
      .as_ref()
    {
      recorder.paused.store(false, Ordering::Relaxed);
    }
    Ok(())
  }

  pub fn get_capabilities(&self) -> Result<Capabilities> {
    Ok(desktop_capabilities())
  }

  pub fn get_supported_formats(&self) -> Result<Vec<AudioFormat>> {
    Ok(desktop_capabilities().formats)
  }

  pub fn check_permissions(&self) -> Result<MicrophonePermissions> {
    Ok(MicrophonePermissions {
      microphone: PermissionState::Prompt,
    })
  }

  pub fn request_permissions(&self) -> Result<MicrophonePermissions> {
    Ok(MicrophonePermissions {
      microphone: PermissionState::Prompt,
    })
  }

  pub fn open_app_settings(&self) -> Result<()> {
    Err(Error::unsupported("openAppSettings on desktop"))
  }

  pub fn start_meter(&self, options: &MeterOptions) -> Result<()> {
    let _ = options;
    Err(Error::unsupported(
      "native desktop metering; use the AnalyserNode in JS",
    ))
  }

  pub fn stop_meter(&self) -> Result<()> {
    Ok(())
  }

  pub fn start_audio_stream(
    &self,
    options: &StreamOptions,
    channel: Channel<InvokeResponseBody>,
  ) -> Result<StreamInfo> {
    let mut guard = self
      .stream
      .lock()
      .map_err(|_| Error::message("stream lock poisoned"))?;
    if guard.is_some() {
      return Err(Error::InvalidState("audio stream already active".into()));
    }

    let host = cpal::default_host();
    let device = resolve_input_device(&host, None)?;
    let default_config = device
      .default_input_config()
      .map_err(|e| Error::message(format!("no input configuration available: {e}")))?;
    let (sample_rate, channels) = resolve_capture_config(
      options.sample_rate,
      options.channels,
      default_config.sample_rate().0,
      default_config.channels(),
    );
    let sample_format = default_config.sample_format();
    let frame_samples = options.frame_samples.filter(|f| *f > 0).unwrap_or(1024);
    let max_seconds = options.max_seconds.filter(|m| *m > 0.0);

    let buffer = Arc::new(Mutex::new(StreamBuffer {
      pending: Vec::new(),
      queue: VecDeque::new(),
      capacity: 64,
      dropped: 0,
      sequence: 0,
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = sync_channel(1);

    let thread_buffer = buffer.clone();
    let thread_stop = stop.clone();
    let handle = std::thread::spawn(move || {
      let stream = match start_capture_stream(
        &device,
        sample_format,
        sample_rate,
        channels,
        frame_samples,
        thread_buffer.clone(),
        thread_stop.clone(),
        max_seconds,
      ) {
        Ok(stream) => match stream.play() {
          Ok(()) => {
            let _ = tx.send(Ok(()));
            stream
          }
          Err(err) => {
            let _ = tx.send(Err(format!("failed to start stream: {err}")));
            return;
          }
        },
        Err(err) => {
          let _ = tx.send(Err(err.to_string()));
          return;
        }
      };

      loop {
        let frame = thread_buffer
          .lock()
          .ok()
          .and_then(|mut buffer| buffer.queue.pop_front());
        match frame {
          Some(bytes) => {
            let _ = channel.send(InvokeResponseBody::Raw(bytes));
          }
          None => {
            if thread_stop.load(Ordering::Relaxed) {
              break;
            }
            std::thread::sleep(Duration::from_millis(2));
          }
        }
      }
      drop(stream);
    });

    match rx.recv() {
      Ok(Ok(())) => {
        *guard = Some(ActiveStream {
          stop,
          thread: Some(handle),
        });
        Ok(StreamInfo {
          transport: StreamTransport::Raw,
          sample_rate,
          channels,
          frame_samples,
        })
      }
      Ok(Err(message)) => {
        let _ = handle.join();
        Err(Error::message(message))
      }
      Err(_) => Err(Error::message("stream thread failed to start")),
    }
  }

  pub fn stop_audio_stream(&self) -> Result<()> {
    let active = self
      .stream
      .lock()
      .map_err(|_| Error::message("stream lock poisoned"))?
      .take();
    if let Some(active) = active {
      active.stop.store(true, Ordering::Relaxed);
      if let Some(thread) = active.thread {
        let _ = thread.join();
      }
    }
    Ok(())
  }

  fn spawn_recorder(&self, options: &RecordingOptions) -> Result<ActiveRecorder> {
    let host = cpal::default_host();
    let device = resolve_input_device(&host, options.device_id.as_deref())?;
    let default_config = device
      .default_input_config()
      .map_err(|e| Error::message(format!("no input configuration available: {e}")))?;

    let (mut sample_rate, mut channels) = resolve_capture_config(
      options.sample_rate,
      options.channels,
      default_config.sample_rate().0,
      default_config.channels(),
    );
    let format = effective_native_format(options.format);
    if format == AudioFormat::Opus {
      sample_rate = opus_sample_rate(sample_rate);
      channels = channels.clamp(1, 2);
    }
    let sample_format = default_config.sample_format();
    let config = cpal::StreamConfig {
      channels,
      sample_rate: cpal::SampleRate(sample_rate),
      buffer_size: cpal::BufferSize::Default,
    };

    let path = std::env::temp_dir().join(format!(
      "tauri-microphone-{}.{}",
      unique_id(),
      format.extension()
    ));
    let sink = match format {
      AudioFormat::Flac => Sink::Pcm(Vec::new()),
      AudioFormat::Opus => Sink::Opus(OpusSink::new(channels, sample_rate)?),
      _ => Sink::Wav(
        hound::WavWriter::create(&path, wav_spec(channels, sample_rate))
          .map_err(|e| Error::message(format!("failed to create WAV file: {e}")))?,
      ),
    };

    let writer = Arc::new(Mutex::new(Some(sink)));
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(AtomicU64::new(0));
    let max_frames = max_frames(options.max_duration, sample_rate);

    let (tx, rx) = sync_channel(1);
    let thread_writer = writer.clone();
    let thread_stop = stop.clone();
    let thread_paused = paused.clone();
    let thread_frames = frames.clone();
    let thread_path = path.clone();

    let handle = std::thread::spawn(move || {
      let stream = match start_capture(
        &device,
        &config,
        sample_format,
        thread_writer.clone(),
        thread_stop.clone(),
        thread_paused.clone(),
        thread_frames.clone(),
        max_frames,
        channels as usize,
      ) {
        Ok(stream) => match stream.play() {
          Ok(()) => {
            let _ = tx.send(Ok(()));
            stream
          }
          Err(err) => {
            let _ = tx.send(Err(format!("failed to start input stream: {err}")));
            return;
          }
        },
        Err(err) => {
          let _ = tx.send(Err(err.to_string()));
          return;
        }
      };

      while !thread_stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
      }
      drop(stream);
      if let Ok(mut guard) = thread_writer.lock() {
        if let Some(sink) = guard.take() {
          match sink {
            Sink::Wav(writer) => {
              let _ = writer.finalize();
            }
            Sink::Pcm(samples) => {
              let _ = encode_flac(&thread_path, &samples, channels, sample_rate);
            }
            Sink::Opus(opus) => {
              let _ = opus.finish(&thread_path);
            }
          }
        }
      }
    });

    match rx.recv() {
      Ok(Ok(())) => Ok(ActiveRecorder {
        stop,
        paused,
        frames,
        thread: Some(handle),
        path,
        sample_rate,
        format: effective_native_format(options.format),
      }),
      Ok(Err(message)) => {
        let _ = handle.join();
        Err(Error::message(message))
      }
      Err(_) => Err(Error::message("recorder thread failed to start")),
    }
  }
}

/// Rejects a background request when the platform cannot honor it.
fn ensure_background_supported(background: Option<bool>, supported: bool) -> Result<()> {
  if background == Some(true) && !supported {
    return Err(Error::unsupported("background recording on this platform"));
  }
  Ok(())
}

/// The guaranteed native desktop path produces WAV, FLAC (pure Rust) and
/// Ogg/Opus (vendored libopus). Other requested formats degrade and are reported
/// back as the effective format.
fn effective_native_format(requested: Option<AudioFormat>) -> AudioFormat {
  match requested {
    Some(AudioFormat::Flac) => AudioFormat::Flac,
    Some(AudioFormat::Opus) => AudioFormat::Opus,
    _ => AudioFormat::Wav,
  }
}

/// Opus only accepts specific input rates; pick the closest supported one.
fn opus_sample_rate(requested: u32) -> u32 {
  const RATES: [u32; 5] = [8000, 12000, 16000, 24000, 48000];
  if RATES.contains(&requested) {
    requested
  } else {
    48_000
  }
}

/// Encodes interleaved 16-bit PCM samples to a FLAC file (pure Rust).
fn encode_flac(path: &Path, samples: &[i16], channels: u16, sample_rate: u32) -> Result<()> {
  use flacenc::component::BitRepr;
  use flacenc::error::Verify;

  let signal: Vec<i32> = samples.iter().map(|&s| i32::from(s)).collect();
  let config = flacenc::config::Encoder::default()
    .into_verified()
    .map_err(|(_, e)| Error::message(format!("FLAC config error: {e:?}")))?;
  let source =
    flacenc::source::MemSource::from_samples(&signal, channels as usize, 16, sample_rate as usize);
  let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
    .map_err(|e| Error::message(format!("FLAC encoding failed: {e}")))?;
  let mut sink = flacenc::bitsink::ByteSink::new();
  stream
    .write(&mut sink)
    .map_err(|e| Error::message(format!("FLAC serialization failed: {e:?}")))?;
  std::fs::write(path, sink.as_slice()).map_err(Error::Io)
}

/// Capabilities advertised by the guaranteed native desktop path.
fn desktop_capabilities() -> Capabilities {
  Capabilities {
    formats: vec![AudioFormat::Wav, AudioFormat::Flac, AudioFormat::Opus],
    preferred_format: AudioFormat::Wav,
    sample_rates: vec![44_100, 48_000],
    channels: vec![1, 2],
    supports_pause: true,
    supports_background: false,
    supports_spectrum: true,
    supports_audio_stream: true,
    stream_transport: StreamTransport::Raw,
  }
}

/// Picks the input device by name, or the default device when no id is given.
fn resolve_input_device(host: &cpal::Host, device_id: Option<&str>) -> Result<cpal::Device> {
  match device_id {
    Some(id) => host
      .input_devices()
      .map_err(|e| Error::message(format!("failed to enumerate input devices: {e}")))?
      .find(|device| device.name().map(|name| name == id).unwrap_or(false))
      .ok_or_else(|| Error::UnknownDevice(id.to_string())),
    None => host
      .default_input_device()
      .ok_or_else(|| Error::message("no default input device available")),
  }
}

/// Resolves the effective capture rate/channels: requested values win, falling
/// back to the device defaults.
fn resolve_capture_config(
  requested_rate: Option<u32>,
  requested_channels: Option<u16>,
  default_rate: u32,
  default_channels: u16,
) -> (u32, u16) {
  (
    requested_rate.filter(|r| *r > 0).unwrap_or(default_rate),
    requested_channels
      .filter(|c| *c > 0)
      .unwrap_or(default_channels),
  )
}

/// Frames that correspond to `max_duration` seconds, or `0` when unlimited.
fn max_frames(max_duration: Option<f64>, sample_rate: u32) -> u64 {
  max_duration
    .filter(|d| *d > 0.0)
    .map(|d| (d * sample_rate as f64) as u64)
    .unwrap_or(0)
}

/// Whether the capture reached its configured duration and must stop.
fn should_auto_stop(total_frames: u64, max_frames: u64) -> bool {
  max_frames > 0 && total_frames >= max_frames
}

fn wav_spec(channels: u16, sample_rate: u32) -> hound::WavSpec {
  hound::WavSpec {
    channels,
    sample_rate,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
  }
}

fn unique_id() -> u128 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn start_capture(
  device: &cpal::Device,
  config: &cpal::StreamConfig,
  sample_format: cpal::SampleFormat,
  writer: Arc<Mutex<Option<Sink>>>,
  stop: Arc<AtomicBool>,
  paused: Arc<AtomicBool>,
  frames: Arc<AtomicU64>,
  max_frames: u64,
  channels: usize,
) -> Result<cpal::Stream> {
  let err_fn = |err: cpal::StreamError| eprintln!("microphone stream error: {err}");

  macro_rules! build_stream {
    ($ty:ty, $convert:expr) => {{
      let writer = writer.clone();
      let stop = stop.clone();
      let paused = paused.clone();
      let frames = frames.clone();
      device
        .build_input_stream(
          config,
          move |data: &[$ty], _: &cpal::InputCallbackInfo| {
            if paused.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
              return;
            }
            let mut guard = match writer.lock() {
              Ok(guard) => guard,
              Err(_) => return,
            };
            let Some(sink) = guard.as_mut() else {
              return;
            };
            let samples: Vec<i16> = data.iter().map(|&s| $convert(s)).collect();
            match sink {
              Sink::Wav(wav) => {
                for sample in samples {
                  let _ = wav.write_sample(sample);
                }
              }
              Sink::Pcm(buffer) => buffer.extend_from_slice(&samples),
              Sink::Opus(opus) => {
                let _ = opus.push(&samples);
              }
            }
            drop(guard);
            let written = (data.len() / channels.max(1)) as u64;
            let total = frames.fetch_add(written, Ordering::Relaxed) + written;
            if should_auto_stop(total, max_frames) {
              stop.store(true, Ordering::Relaxed);
            }
          },
          err_fn,
          None,
        )
        .map_err(|e| Error::message(format!("failed to build input stream: {e}")))
    }};
  }

  match sample_format {
    cpal::SampleFormat::F32 => {
      build_stream!(f32, |s: f32| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
    }
    cpal::SampleFormat::I16 => build_stream!(i16, |s: i16| s),
    cpal::SampleFormat::U16 => build_stream!(u16, |s: u16| (s as i32 - 32_768) as i16),
    other => Err(Error::unsupported(format!("input sample format {other:?}"))),
  }
}

const STREAM_HEADER_BYTES: usize = 16; // [sequence: u64][timestamp: f64]

/// Builds a binary stream frame: `[sequence: u64 LE][timestamp: f64 LE][pcm: i16 LE ..]`.
fn build_frame(sequence: u64, timestamp: f64, pcm: &[i16]) -> Vec<u8> {
  let mut bytes = Vec::with_capacity(STREAM_HEADER_BYTES + pcm.len() * 2);
  bytes.extend_from_slice(&sequence.to_le_bytes());
  bytes.extend_from_slice(&timestamp.to_le_bytes());
  for sample in pcm {
    bytes.extend_from_slice(&sample.to_le_bytes());
  }
  bytes
}

/// Enqueues a frame, dropping the oldest one when the bounded buffer is full.
fn enqueue_frame(buffer: &mut StreamBuffer, bytes: Vec<u8>) {
  if buffer.queue.len() >= buffer.capacity {
    buffer.queue.pop_front();
    buffer.dropped += 1;
  }
  buffer.queue.push_back(bytes);
}

/// Decodes a binary stream frame back into `(sequence, timestamp, pcm)`.
#[cfg(test)]
fn decode_frame(bytes: &[u8]) -> Option<(u64, f64, Vec<i16>)> {
  if bytes.len() < STREAM_HEADER_BYTES {
    return None;
  }
  let sequence = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
  let timestamp = f64::from_le_bytes(bytes[8..16].try_into().ok()?);
  let pcm = bytes[STREAM_HEADER_BYTES..]
    .chunks_exact(2)
    .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
    .collect();
  Some((sequence, timestamp, pcm))
}

#[allow(clippy::too_many_arguments)]
fn start_capture_stream(
  device: &cpal::Device,
  sample_format: cpal::SampleFormat,
  sample_rate: u32,
  channels: u16,
  frame_samples: u32,
  buffer: Arc<Mutex<StreamBuffer>>,
  stop: Arc<AtomicBool>,
  max_seconds: Option<f64>,
) -> Result<cpal::Stream> {
  let config = cpal::StreamConfig {
    channels,
    sample_rate: cpal::SampleRate(sample_rate),
    buffer_size: cpal::BufferSize::Default,
  };
  let err_fn = |err: cpal::StreamError| eprintln!("microphone stream error: {err}");
  let frame_len = frame_samples as usize * channels.max(1) as usize;
  let max_frames = max_seconds
    .map(|seconds| (seconds * sample_rate as f64) as u64)
    .unwrap_or(0);

  macro_rules! build_stream {
    ($ty:ty, $convert:expr) => {{
      let buffer = buffer.clone();
      let stop = stop.clone();
      let start = Instant::now();
      device
        .build_input_stream(
          &config,
          move |data: &[$ty], _: &cpal::InputCallbackInfo| {
            if stop.load(Ordering::Relaxed) {
              return;
            }
            let mut state = match buffer.lock() {
              Ok(state) => state,
              Err(_) => return,
            };
            for &sample in data {
              let value: i16 = $convert(sample);
              state.pending.push(value);
            }
            while state.pending.len() >= frame_len {
              let frame: Vec<i16> = state.pending.drain(0..frame_len).collect();
              let timestamp = start.elapsed().as_secs_f64();
              let bytes = build_frame(state.sequence, timestamp, &frame);
              state.sequence += 1;
              enqueue_frame(&mut state, bytes);
              if max_frames > 0 && state.sequence * u64::from(frame_samples) >= max_frames {
                stop.store(true, Ordering::Relaxed);
              }
            }
          },
          err_fn,
          None,
        )
        .map_err(|e| Error::message(format!("failed to build stream: {e}")))
    }};
  }

  match sample_format {
    cpal::SampleFormat::F32 => {
      build_stream!(f32, |s: f32| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
    }
    cpal::SampleFormat::I16 => build_stream!(i16, |s: i16| s),
    cpal::SampleFormat::U16 => build_stream!(u16, |s: u16| (s as i32 - 32_768) as i16),
    other => Err(Error::unsupported(format!("input sample format {other:?}"))),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn resolve_capture_config_prefers_requested() {
    assert_eq!(
      resolve_capture_config(Some(44_100), Some(2), 48_000, 1),
      (44_100, 2)
    );
  }

  #[test]
  fn resolve_capture_config_falls_back_to_defaults() {
    assert_eq!(resolve_capture_config(None, None, 48_000, 1), (48_000, 1));
    assert_eq!(
      resolve_capture_config(Some(0), Some(0), 48_000, 1),
      (48_000, 1)
    );
  }

  #[test]
  fn native_desktop_resolves_wav_flac_and_opus_else_degrades() {
    assert_eq!(effective_native_format(None), AudioFormat::Wav);
    assert_eq!(
      effective_native_format(Some(AudioFormat::Wav)),
      AudioFormat::Wav
    );
    assert_eq!(
      effective_native_format(Some(AudioFormat::Flac)),
      AudioFormat::Flac
    );
    assert_eq!(
      effective_native_format(Some(AudioFormat::Opus)),
      AudioFormat::Opus
    );
    assert_eq!(
      effective_native_format(Some(AudioFormat::Aac)),
      AudioFormat::Wav
    );
    assert_eq!(
      effective_native_format(Some(AudioFormat::M4a)),
      AudioFormat::Wav
    );
  }

  #[test]
  fn opus_sample_rate_coerces_to_supported() {
    assert_eq!(opus_sample_rate(48_000), 48_000);
    assert_eq!(opus_sample_rate(16_000), 16_000);
    assert_eq!(opus_sample_rate(44_100), 48_000);
  }

  #[test]
  fn encodes_a_valid_ogg_opus_file() {
    let path = std::env::temp_dir().join(format!("tauri-microphone-test-{}.opus", unique_id()));
    let mut sink = OpusSink::new(1, 48_000).unwrap();
    let samples: Vec<i16> = (0..9_600).map(|i| ((i % 100) as i16) * 100).collect();
    sink.push(&samples).unwrap();
    sink.finish(&path).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"OggS"));
    assert!(bytes.windows(8).any(|window| window == b"OpusHead"));
    assert!(bytes.windows(8).any(|window| window == b"OpusTags"));
    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn encodes_a_valid_flac_file() {
    let path = std::env::temp_dir().join(format!("tauri-microphone-test-{}.flac", unique_id()));
    let samples: Vec<i16> = (0..4096).map(|i| ((i % 100) as i16) * 50).collect();
    encode_flac(&path, &samples, 1, 48_000).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"fLaC"));
    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn rejects_background_when_unsupported() {
    assert!(ensure_background_supported(Some(true), false).is_err());
    assert!(ensure_background_supported(Some(true), true).is_ok());
    assert!(ensure_background_supported(None, false).is_ok());
    assert!(ensure_background_supported(Some(false), false).is_ok());
  }

  #[test]
  fn stream_frame_round_trips() {
    let pcm = [1_i16, -2, 3, -4];
    let bytes = build_frame(7, 1.25, &pcm);
    assert_eq!(bytes.len(), STREAM_HEADER_BYTES + pcm.len() * 2);
    let (sequence, timestamp, decoded) = decode_frame(&bytes).unwrap();
    assert_eq!(sequence, 7);
    assert_eq!(timestamp, 1.25);
    assert_eq!(decoded, pcm.to_vec());
  }

  #[test]
  fn stream_frame_rejects_short_input() {
    assert!(decode_frame(&[0_u8; 4]).is_none());
  }

  #[test]
  fn backpressure_drops_oldest_frames() {
    let mut buffer = StreamBuffer {
      pending: Vec::new(),
      queue: VecDeque::new(),
      capacity: 2,
      dropped: 0,
      sequence: 0,
    };
    enqueue_frame(&mut buffer, vec![1]);
    enqueue_frame(&mut buffer, vec![2]);
    enqueue_frame(&mut buffer, vec![3]);
    assert_eq!(buffer.dropped, 1);
    assert_eq!(buffer.queue.len(), 2);
    assert_eq!(buffer.queue.front().unwrap(), &vec![2_u8]);
  }

  #[test]
  fn desktop_capabilities_are_correct() {
    let caps = desktop_capabilities();
    assert_eq!(
      caps.formats,
      vec![AudioFormat::Wav, AudioFormat::Flac, AudioFormat::Opus]
    );
    assert_eq!(caps.preferred_format, AudioFormat::Wav);
    assert!(caps.supports_pause);
    assert!(!caps.supports_background);
    assert!(caps.supports_audio_stream);
    assert_eq!(caps.stream_transport, StreamTransport::Raw);
    assert!(caps.sample_rates.contains(&48_000));
    assert!(caps.channels.contains(&1));
  }

  #[test]
  fn max_frames_maps_seconds_to_frames() {
    assert_eq!(max_frames(Some(2.0), 48_000), 96_000);
    assert_eq!(max_frames(None, 48_000), 0);
    assert_eq!(max_frames(Some(0.0), 48_000), 0);
    assert_eq!(max_frames(Some(-1.0), 48_000), 0);
  }

  #[test]
  fn auto_stop_triggers_only_at_the_limit() {
    assert!(!should_auto_stop(95_999, 96_000));
    assert!(should_auto_stop(96_000, 96_000));
    assert!(should_auto_stop(96_001, 96_000));
    assert!(!should_auto_stop(96_001, 0));
  }

  #[test]
  fn unknown_device_error_is_descriptive() {
    let error = Error::UnknownDevice("nope".to_string());
    assert_eq!(error.to_string(), "unknown input device: nope");
  }

  #[test]
  fn wav_spec_is_16_bit_pcm() {
    let spec = wav_spec(2, 44_100);
    assert_eq!(spec.channels, 2);
    assert_eq!(spec.sample_rate, 44_100);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
  }

  #[test]
  fn writes_a_valid_wav_header() {
    let path = std::env::temp_dir().join(format!("tauri-microphone-test-{}.wav", unique_id()));
    let spec = wav_spec(1, 48_000);
    {
      let mut writer = hound::WavWriter::create(&path, spec).unwrap();
      for sample in [0_i16, 1000, -1000, i16::MAX, i16::MIN] {
        writer.write_sample(sample).unwrap();
      }
      writer.finalize().unwrap();
    }
    let reader = hound::WavReader::open(&path).unwrap();
    let reader_spec = reader.spec();
    assert_eq!(reader_spec.channels, 1);
    assert_eq!(reader_spec.sample_rate, 48_000);
    assert_eq!(reader_spec.bits_per_sample, 16);
    let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(samples, vec![0, 1000, -1000, i16::MAX, i16::MIN]);
    let _ = std::fs::remove_file(&path);
  }
}

use serde::{Deserialize, Serialize};

/// Audio output format. WAV/PCM is guaranteed on every platform; the other
/// formats are produced progressively and reported back as the effective format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
  #[default]
  Wav,
  M4a,
  Aac,
  Flac,
  Opus,
}

impl AudioFormat {
  /// Lowercase identifier used on the wire and in `AudioClip.format`.
  pub fn as_str(&self) -> &'static str {
    match self {
      AudioFormat::Wav => "wav",
      AudioFormat::M4a => "m4a",
      AudioFormat::Aac => "aac",
      AudioFormat::Flac => "flac",
      AudioFormat::Opus => "opus",
    }
  }

  /// File extension for the format.
  pub fn extension(&self) -> &'static str {
    match self {
      AudioFormat::Wav => "wav",
      AudioFormat::M4a => "m4a",
      AudioFormat::Aac => "aac",
      AudioFormat::Flac => "flac",
      AudioFormat::Opus => "opus",
    }
  }

  /// MIME type for the format.
  pub fn mime_type(&self) -> &'static str {
    match self {
      AudioFormat::Wav => "audio/wav",
      AudioFormat::M4a => "audio/mp4",
      AudioFormat::Aac => "audio/aac",
      AudioFormat::Flac => "audio/flac",
      AudioFormat::Opus => "audio/ogg",
    }
  }
}

/// Current recording state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingState {
  #[default]
  Idle,
  Recording,
  Paused,
}

/// Result of an OS permission query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionState {
  Granted,
  Denied,
  Prompt,
}

/// Scale used for the metering level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MeterScale {
  Linear,
  Dbfs,
}

/// Options accepted by `startRecording`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingOptions {
  /// Preferred output format. The effective format is reported in `AudioClip`.
  pub format: Option<AudioFormat>,
  pub sample_rate: Option<u32>,
  pub channels: Option<u16>,
  pub bit_rate: Option<u32>,
  /// Automatic stop after this many seconds.
  pub max_duration: Option<f64>,
  /// Desktop input device identifier.
  pub device_id: Option<String>,
  /// Request continued recording in the background (mobile only).
  pub background: Option<bool>,
}

impl RecordingOptions {
  pub fn sample_rate_or_default(&self) -> u32 {
    self.sample_rate.unwrap_or(48_000)
  }

  pub fn channels_or_default(&self) -> u16 {
    self.channels.unwrap_or(1)
  }
}

/// Result of a recording. On mobile `path` is set; on desktop the webview
/// facade adds `blob`/`url`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
  pub duration: f64,
  pub format: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub path: Option<String>,
}

/// Options accepted by `onMeter`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterOptions {
  pub interval_ms: Option<u32>,
  pub scale: Option<MeterScale>,
  pub floor_db: Option<f64>,
  pub peak_hold: Option<bool>,
  pub spectrum: Option<bool>,
}

/// Metering payload emitted to the webview.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterData {
  pub level: f64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub peak: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub rms: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub db: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub clipping: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub spectrum: Option<Vec<f64>>,
  pub timestamp: f64,
}

/// Transport used to deliver raw audio frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamTransport {
  #[default]
  Raw,
  Base64,
}

/// Options accepted by `startAudioStream`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamOptions {
  pub sample_rate: Option<u32>,
  pub channels: Option<u16>,
  pub frame_samples: Option<u32>,
  pub max_seconds: Option<f64>,
}

/// Information returned by `startAudioStream`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
  pub transport: StreamTransport,
  pub sample_rate: u32,
  pub channels: u16,
  pub frame_samples: u32,
}

/// A normalized PCM frame delivered over the stream `Channel`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioChunk {
  /// Base64 PCM payload (mobile transport). Desktop sends raw bytes instead.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub data: Option<String>,
  pub sequence: u64,
  pub timestamp: f64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub dropped_before: Option<u64>,
}

/// Formats, rates and features supported by the current platform/device.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
  pub formats: Vec<AudioFormat>,
  pub preferred_format: AudioFormat,
  pub sample_rates: Vec<u32>,
  pub channels: Vec<u16>,
  pub supports_pause: bool,
  pub supports_background: bool,
  pub supports_spectrum: bool,
  pub supports_audio_stream: bool,
  pub stream_transport: StreamTransport,
}

/// Widget-free permission response returned to the facade.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophonePermissions {
  pub microphone: PermissionState,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn audio_format_serializes_lowercase() {
    assert_eq!(
      serde_json::to_string(&AudioFormat::Wav).unwrap(),
      r#""wav""#
    );
    assert_eq!(
      serde_json::to_string(&AudioFormat::M4a).unwrap(),
      r#""m4a""#
    );
    assert_eq!(
      serde_json::to_string(&AudioFormat::Opus).unwrap(),
      r#""opus""#
    );
  }

  #[test]
  fn recording_state_defaults_to_idle_and_lowercase() {
    assert_eq!(RecordingState::default(), RecordingState::Idle);
    assert_eq!(
      serde_json::to_string(&RecordingState::Paused).unwrap(),
      r#""paused""#
    );
  }

  #[test]
  fn recording_options_deserialize_camel_case() {
    let opts: RecordingOptions = serde_json::from_str(
      r#"{"format":"flac","sampleRate":44100,"channels":2,"maxDuration":10.5,"deviceId":"mic-1"}"#,
    )
    .unwrap();
    assert_eq!(opts.format, Some(AudioFormat::Flac));
    assert_eq!(opts.sample_rate, Some(44_100));
    assert_eq!(opts.channels, Some(2));
    assert_eq!(opts.max_duration, Some(10.5));
    assert_eq!(opts.device_id.as_deref(), Some("mic-1"));
  }

  #[test]
  fn audio_clip_omits_absent_path() {
    let clip = AudioClip {
      duration: 1.5,
      format: "wav".into(),
      path: None,
    };
    assert_eq!(
      serde_json::to_string(&clip).unwrap(),
      r#"{"duration":1.5,"format":"wav"}"#
    );
  }

  #[test]
  fn audio_clip_deserializes_mobile_shape() {
    let clip: AudioClip =
      serde_json::from_str(r#"{"path":"/tmp/a.wav","duration":2.0,"format":"wav"}"#).unwrap();
    assert_eq!(clip.path.as_deref(), Some("/tmp/a.wav"));
    assert_eq!(clip.duration, 2.0);
  }

  #[test]
  fn meter_data_omits_optional_fields() {
    let data = MeterData {
      level: 0.5,
      timestamp: 12.0,
      ..Default::default()
    };
    assert_eq!(
      serde_json::to_string(&data).unwrap(),
      r#"{"level":0.5,"timestamp":12.0}"#
    );
  }

  #[test]
  fn capabilities_round_trips() {
    let caps = Capabilities {
      formats: vec![AudioFormat::Wav, AudioFormat::Opus],
      preferred_format: AudioFormat::Wav,
      sample_rates: vec![44_100, 48_000],
      channels: vec![1, 2],
      supports_pause: true,
      supports_background: false,
      supports_spectrum: true,
      supports_audio_stream: true,
      stream_transport: StreamTransport::Raw,
    };
    let json = serde_json::to_string(&caps).unwrap();
    let back: Capabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(back.preferred_format, AudioFormat::Wav);
    assert_eq!(back.stream_transport, StreamTransport::Raw);
  }
}

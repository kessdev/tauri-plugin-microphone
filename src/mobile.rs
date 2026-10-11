use serde::de::DeserializeOwned;
use tauri::{
  ipc::{Channel, InvokeResponseBody},
  plugin::{PluginApi, PluginHandle},
  AppHandle, Runtime,
};

use crate::error::Result;
use crate::models::*;

#[derive(serde::Serialize)]
struct StartStreamArgs<'a> {
  options: &'a StreamOptions,
  channel: Channel<InvokeResponseBody>,
}

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "app.tauri.microphone";

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_microphone);

// Initializes the Kotlin or Swift plugin classes.
pub fn init<R: Runtime, C: DeserializeOwned>(
  _app: &AppHandle<R>,
  api: PluginApi<R, C>,
) -> crate::Result<Microphone<R>> {
  #[cfg(target_os = "android")]
  let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "MicrophonePlugin")?;
  #[cfg(target_os = "ios")]
  let handle = api.register_ios_plugin(init_plugin_microphone)?;
  Ok(Microphone(handle))
}

/// Access to the microphone APIs on mobile; every call is delegated to the
/// native backend.
pub struct Microphone<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Microphone<R> {
  fn call<T: DeserializeOwned>(&self, command: &str, payload: impl serde::Serialize) -> Result<T> {
    self
      .0
      .run_mobile_plugin(command, payload)
      .map_err(Into::into)
  }

  pub fn start_recording(&self, options: &RecordingOptions) -> Result<()> {
    self.call("start_recording", options)
  }

  pub fn stop_recording(&self) -> Result<AudioClip> {
    self.call("stop_recording", ())
  }

  pub fn pause_recording(&self) -> Result<()> {
    self.call("pause_recording", ())
  }

  pub fn resume_recording(&self) -> Result<()> {
    self.call("resume_recording", ())
  }

  pub fn get_recording_state(&self) -> Result<RecordingState> {
    self.call("get_recording_state", ())
  }

  pub fn get_capabilities(&self) -> Result<Capabilities> {
    self.call("get_capabilities", ())
  }

  pub fn get_supported_formats(&self) -> Result<Vec<AudioFormat>> {
    self.call("get_supported_formats", ())
  }

  pub fn check_permissions(&self) -> Result<MicrophonePermissions> {
    self.call("check_permissions", ())
  }

  pub fn request_permissions(&self) -> Result<MicrophonePermissions> {
    self.call("request_permissions", ())
  }

  pub fn open_app_settings(&self) -> Result<()> {
    self.call("open_app_settings", ())
  }

  pub fn start_meter(&self, options: &MeterOptions) -> Result<()> {
    self.call("start_meter", options)
  }

  pub fn stop_meter(&self) -> Result<()> {
    self.call("stop_meter", ())
  }

  pub fn start_audio_stream(
    &self,
    options: &StreamOptions,
    channel: Channel<InvokeResponseBody>,
  ) -> Result<StreamInfo> {
    self.call("start_audio_stream", StartStreamArgs { options, channel })
  }

  pub fn stop_audio_stream(&self) -> Result<()> {
    self.call("stop_audio_stream", ())
  }
}

use tauri::{
  command,
  ipc::{Channel, InvokeResponseBody},
  AppHandle, Runtime,
};

use crate::models::*;
use crate::MicrophoneExt;
use crate::Result;

#[command]
pub(crate) async fn start_recording<R: Runtime>(
  app: AppHandle<R>,
  options: Option<RecordingOptions>,
) -> Result<()> {
  app
    .microphone()
    .start_recording(&options.unwrap_or_default())
}

#[command]
pub(crate) async fn stop_recording<R: Runtime>(app: AppHandle<R>) -> Result<AudioClip> {
  app.microphone().stop_recording()
}

#[command]
pub(crate) async fn pause_recording<R: Runtime>(app: AppHandle<R>) -> Result<()> {
  app.microphone().pause_recording()
}

#[command]
pub(crate) async fn resume_recording<R: Runtime>(app: AppHandle<R>) -> Result<()> {
  app.microphone().resume_recording()
}

#[command]
pub(crate) async fn get_recording_state<R: Runtime>(app: AppHandle<R>) -> Result<RecordingState> {
  app.microphone().get_recording_state()
}

#[command]
pub(crate) async fn get_capabilities<R: Runtime>(app: AppHandle<R>) -> Result<Capabilities> {
  app.microphone().get_capabilities()
}

#[command]
pub(crate) async fn get_supported_formats<R: Runtime>(
  app: AppHandle<R>,
) -> Result<Vec<AudioFormat>> {
  app.microphone().get_supported_formats()
}

#[command]
pub(crate) async fn check_permissions<R: Runtime>(
  app: AppHandle<R>,
) -> Result<MicrophonePermissions> {
  app.microphone().check_permissions()
}

#[command]
pub(crate) async fn request_permissions<R: Runtime>(
  app: AppHandle<R>,
) -> Result<MicrophonePermissions> {
  app.microphone().request_permissions()
}

#[command]
pub(crate) async fn open_app_settings<R: Runtime>(app: AppHandle<R>) -> Result<()> {
  app.microphone().open_app_settings()
}

#[command]
pub(crate) async fn start_meter<R: Runtime>(
  app: AppHandle<R>,
  options: Option<MeterOptions>,
) -> Result<()> {
  app.microphone().start_meter(&options.unwrap_or_default())
}

#[command]
pub(crate) async fn stop_meter<R: Runtime>(app: AppHandle<R>) -> Result<()> {
  app.microphone().stop_meter()
}

#[command]
pub(crate) async fn start_audio_stream<R: Runtime>(
  app: AppHandle<R>,
  options: Option<StreamOptions>,
  channel: Channel<InvokeResponseBody>,
) -> Result<StreamInfo> {
  app
    .microphone()
    .start_audio_stream(&options.unwrap_or_default(), channel)
}

#[command]
pub(crate) async fn stop_audio_stream<R: Runtime>(app: AppHandle<R>) -> Result<()> {
  app.microphone().stop_audio_stream()
}

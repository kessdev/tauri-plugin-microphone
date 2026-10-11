const COMMANDS: &[&str] = &[
  "start_recording",
  "stop_recording",
  "pause_recording",
  "resume_recording",
  "get_recording_state",
  "get_capabilities",
  "get_supported_formats",
  "check_permissions",
  "request_permissions",
  "open_app_settings",
  "start_meter",
  "stop_meter",
  "start_audio_stream",
  "stop_audio_stream",
  "register_listener",
  "remove_listener",
];

fn main() {
  tauri_plugin::Builder::new(COMMANDS)
    .android_path("android")
    .ios_path("ios")
    .build();
}

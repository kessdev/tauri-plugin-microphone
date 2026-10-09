use tauri::{
  plugin::{Builder, TauriPlugin},
  Manager, Runtime,
};

pub use models::*;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::Microphone;
#[cfg(mobile)]
use mobile::Microphone;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to access the microphone APIs.
pub trait MicrophoneExt<R: Runtime> {
  fn microphone(&self) -> &Microphone<R>;
}

impl<R: Runtime, T: Manager<R>> crate::MicrophoneExt<R> for T {
  fn microphone(&self) -> &Microphone<R> {
    self.state::<Microphone<R>>().inner()
  }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("microphone")
    .invoke_handler(tauri::generate_handler![commands::ping])
    .setup(|app, api| {
      #[cfg(mobile)]
      let microphone = mobile::init(app, api)?;
      #[cfg(desktop)]
      let microphone = desktop::init(app, api)?;
      app.manage(microphone);
      Ok(())
    })
    .build()
}

use serde::{ser::Serializer, Serialize};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error(transparent)]
  Io(#[from] std::io::Error),

  #[error("invalid recording state: {0}")]
  InvalidState(String),

  #[error("unsupported on this platform: {0}")]
  Unsupported(String),

  #[error("unknown input device: {0}")]
  UnknownDevice(String),

  #[error("microphone permission denied: {0}")]
  PermissionDenied(String),

  #[error("{0}")]
  Message(String),

  #[cfg(mobile)]
  #[error(transparent)]
  PluginInvoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

impl Error {
  pub fn message(message: impl Into<String>) -> Self {
    Error::Message(message.into())
  }

  pub fn unsupported(feature: impl Into<String>) -> Self {
    Error::Unsupported(feature.into())
  }
}

impl Serialize for Error {
  fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.to_string().as_ref())
  }
}

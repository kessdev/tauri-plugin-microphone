use crate::error::{Error, Result};
use crate::models::RecordingState;

/// Pure recording state machine shared by every backend.
///
/// Transitions: `idle -> recording -> paused -> recording -> idle`. Invalid
/// transitions are rejected with a descriptive [`Error::InvalidState`].
#[derive(Debug, Clone, Default)]
pub struct RecordingStateMachine {
  state: RecordingState,
}

impl RecordingStateMachine {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn state(&self) -> RecordingState {
    self.state
  }

  pub fn start(&mut self) -> Result<RecordingState> {
    if self.state != RecordingState::Idle {
      return Err(Error::InvalidState(format!(
        "cannot start while {}",
        self.state_word()
      )));
    }
    self.state = RecordingState::Recording;
    Ok(self.state)
  }

  pub fn stop(&mut self) -> Result<RecordingState> {
    if self.state == RecordingState::Idle {
      return Err(Error::InvalidState("cannot stop while idle".into()));
    }
    self.state = RecordingState::Idle;
    Ok(self.state)
  }

  pub fn pause(&mut self) -> Result<RecordingState> {
    if self.state != RecordingState::Recording {
      return Err(Error::InvalidState(format!(
        "cannot pause while {}",
        self.state_word()
      )));
    }
    self.state = RecordingState::Paused;
    Ok(self.state)
  }

  pub fn resume(&mut self) -> Result<RecordingState> {
    if self.state != RecordingState::Paused {
      return Err(Error::InvalidState(format!(
        "cannot resume while {}",
        self.state_word()
      )));
    }
    self.state = RecordingState::Recording;
    Ok(self.state)
  }

  /// Returns to idle after an unexpected stop (e.g. max duration or error).
  pub fn force_idle(&mut self) {
    self.state = RecordingState::Idle;
  }

  fn state_word(&self) -> &'static str {
    match self.state {
      RecordingState::Idle => "idle",
      RecordingState::Recording => "recording",
      RecordingState::Paused => "paused",
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lifecycle_start_stop() {
    let mut sm = RecordingStateMachine::new();
    assert_eq!(sm.state(), RecordingState::Idle);
    assert_eq!(sm.start().unwrap(), RecordingState::Recording);
    assert_eq!(sm.stop().unwrap(), RecordingState::Idle);
  }

  #[test]
  fn lifecycle_pause_resume() {
    let mut sm = RecordingStateMachine::new();
    sm.start().unwrap();
    assert_eq!(sm.pause().unwrap(), RecordingState::Paused);
    assert_eq!(sm.resume().unwrap(), RecordingState::Recording);
    sm.stop().unwrap();
    assert_eq!(sm.state(), RecordingState::Idle);
  }

  #[test]
  fn rejects_stop_while_idle() {
    let mut sm = RecordingStateMachine::new();
    assert!(matches!(sm.stop(), Err(Error::InvalidState(_))));
  }

  #[test]
  fn rejects_start_while_recording() {
    let mut sm = RecordingStateMachine::new();
    sm.start().unwrap();
    assert!(matches!(sm.start(), Err(Error::InvalidState(_))));
  }

  #[test]
  fn rejects_pause_while_idle() {
    let mut sm = RecordingStateMachine::new();
    assert!(matches!(sm.pause(), Err(Error::InvalidState(_))));
  }

  #[test]
  fn rejects_resume_while_recording() {
    let mut sm = RecordingStateMachine::new();
    sm.start().unwrap();
    assert!(matches!(sm.resume(), Err(Error::InvalidState(_))));
  }

  #[test]
  fn force_idle_resets_any_state() {
    let mut sm = RecordingStateMachine::new();
    sm.start().unwrap();
    sm.pause().unwrap();
    sm.force_idle();
    assert_eq!(sm.state(), RecordingState::Idle);
  }
}

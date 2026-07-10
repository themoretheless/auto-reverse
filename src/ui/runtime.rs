//! Lifecycle coordinator for the in-process CGEventTap thread.
//!
//! The settings UI owns this small state machine and asks it to start or
//! poll. Platform installation stays in `platform::macos::event_tap`; the UI
//! no longer carries a loose combination of attempted/running/error flags.

use std::sync::{Arc, RwLock, mpsc};

use crate::config::AppConfig;
use crate::platform::macos::event_tap::{self, TapRunOutcome};
use crate::runtime::RuntimeControl;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum State {
    #[default]
    Idle,
    WaitingForPermissions,
    Starting,
    Running,
    AlreadyRunning,
    Stopped,
    Failed(String),
}

impl State {
    pub(super) fn error_message(&self) -> Option<&str> {
        match self {
            Self::AlreadyRunning => {
                Some("another Auto Reverse instance already owns the event tap")
            }
            Self::Stopped => Some("the event tap run loop stopped unexpectedly"),
            Self::Failed(error) => Some(error),
            Self::Idle | Self::WaitingForPermissions | Self::Starting | Self::Running => None,
        }
    }

    pub(super) fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    fn can_start_automatically(&self) -> bool {
        matches!(self, Self::Idle | Self::WaitingForPermissions)
    }

    pub(super) fn can_retry(&self) -> bool {
        matches!(self, Self::AlreadyRunning | Self::Stopped | Self::Failed(_))
    }
}

enum Event {
    Started,
    Finished(Result<TapRunOutcome, String>),
}

#[derive(Default)]
pub(super) struct TapRuntime {
    state: State,
    events: Option<mpsc::Receiver<Event>>,
}

impl TapRuntime {
    pub(super) fn state(&self) -> &State {
        &self.state
    }

    pub(super) fn start_if_ready(
        &mut self,
        permissions_ready: bool,
        shared_config: Arc<RwLock<AppConfig>>,
        runtime_control: Arc<RuntimeControl>,
    ) {
        if !permissions_ready {
            if !self.state.is_running() {
                self.state = State::WaitingForPermissions;
            }
            return;
        }
        if !self.state.can_start_automatically() {
            return;
        }

        self.spawn(shared_config, runtime_control);
    }

    pub(super) fn retry(
        &mut self,
        shared_config: Arc<RwLock<AppConfig>>,
        runtime_control: Arc<RuntimeControl>,
    ) {
        if !self.state.can_retry() {
            return;
        }
        self.spawn(shared_config, runtime_control);
    }

    fn spawn(
        &mut self,
        shared_config: Arc<RwLock<AppConfig>>,
        runtime_control: Arc<RuntimeControl>,
    ) {
        let (events_tx, events_rx) = mpsc::channel();
        self.state = State::Starting;
        self.events = Some(events_rx);

        std::thread::spawn(move || {
            let started_tx = events_tx.clone();
            let outcome =
                event_tap::install_and_run_with_ready(shared_config, runtime_control, move || {
                    let _ = started_tx.send(Event::Started);
                })
                .map_err(|error| error.to_string());
            let _ = events_tx.send(Event::Finished(outcome));
        });
    }

    pub(super) fn poll(&mut self) {
        let mut disconnected = false;
        if let Some(events) = &self.events {
            loop {
                match events.try_recv() {
                    Ok(event) => self.state = state_after_event(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        if disconnected {
            self.events = None;
            if matches!(self.state, State::Starting) {
                self.state = State::Failed(
                    "event tap thread ended before reporting its startup state".to_string(),
                );
            }
        }
    }

    pub(super) fn wait_for_permissions(&mut self) {
        if !self.state.is_running() && !matches!(self.state, State::Starting) {
            self.state = State::WaitingForPermissions;
        }
    }

    pub(super) fn disabled(&mut self) {
        // A thread in Starting can still acquire the lock and report
        // Started after this call. Keep its receiver/state just like a
        // Running pass-through tap; dropping it here would make a later
        // enable spawn a second thread that only reports AlreadyRunning.
        if matches!(self.state, State::Starting | State::Running) {
            return;
        }
        self.state = State::Idle;
        self.events = None;
    }
}

fn state_after_event(event: Event) -> State {
    match event {
        Event::Started => State::Running,
        Event::Finished(Ok(TapRunOutcome::AlreadyRunning)) => State::AlreadyRunning,
        Event::Finished(Ok(TapRunOutcome::Stopped)) => State::Stopped,
        Event::Finished(Err(error)) => State::Failed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_events_have_distinct_states() {
        assert_eq!(state_after_event(Event::Started), State::Running);
        assert_eq!(
            state_after_event(Event::Finished(Ok(TapRunOutcome::AlreadyRunning))),
            State::AlreadyRunning
        );
        assert_eq!(
            state_after_event(Event::Finished(Ok(TapRunOutcome::Stopped))),
            State::Stopped
        );
        assert_eq!(
            state_after_event(Event::Finished(Err("denied".to_string()))),
            State::Failed("denied".to_string())
        );
    }

    #[test]
    fn disabling_keeps_a_live_pass_through_tap_but_resets_failed_state() {
        let mut live = TapRuntime {
            state: State::Running,
            events: None,
        };
        live.disabled();
        assert_eq!(live.state, State::Running);

        let mut starting = TapRuntime {
            state: State::Starting,
            events: None,
        };
        starting.disabled();
        assert_eq!(starting.state, State::Starting);

        let mut failed = TapRuntime {
            state: State::Failed("denied".to_string()),
            events: None,
        };
        failed.disabled();
        assert_eq!(failed.state, State::Idle);
    }
}

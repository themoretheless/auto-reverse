//! Lifecycle coordinator for the in-process CGEventTap thread.
//!
//! The settings UI owns this small state machine and asks it to start or
//! poll. Platform installation stays in `platform::macos::event_tap`; the UI
//! no longer carries a loose combination of attempted/running/error flags.

use std::sync::{Arc, RwLock, mpsc};
use std::time::{Duration, Instant};

use crate::config::AppConfig;
use crate::platform::macos::event_tap::{self, TapRunOutcome};
use crate::runtime::RuntimeControl;

const WAKE_RECOVERY_WINDOW: Duration = Duration::from_secs(5);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WakeRecoveryAction {
    None,
    Rearm,
    Restart,
}

/// A wake can race with the event-tap run loop returning. Keep a short
/// recovery window open after the notification: first re-arm a live port,
/// then allow exactly one restart if the typed lifecycle reports that the run
/// loop actually stopped. The window prevents a permanent retry loop.
#[derive(Default)]
struct WakeRecovery {
    deadline: Option<Instant>,
    rearm_attempted: bool,
    restart_attempted: bool,
}

impl WakeRecovery {
    fn request(&mut self, now: Instant) {
        self.deadline = Some(now + WAKE_RECOVERY_WINDOW);
        self.rearm_attempted = false;
        self.restart_attempted = false;
    }

    fn cancel(&mut self) {
        *self = Self::default();
    }

    fn next_action(
        &mut self,
        state: &State,
        permissions_ready: bool,
        now: Instant,
    ) -> WakeRecoveryAction {
        let Some(deadline) = self.deadline else {
            return WakeRecoveryAction::None;
        };
        if now >= deadline {
            self.cancel();
            return WakeRecoveryAction::None;
        }
        if !permissions_ready {
            return WakeRecoveryAction::None;
        }

        match state {
            State::Running if !self.rearm_attempted => {
                self.rearm_attempted = true;
                WakeRecoveryAction::Rearm
            }
            State::Idle | State::WaitingForPermissions | State::Stopped | State::Failed(_)
                if !self.restart_attempted =>
            {
                self.restart_attempted = true;
                // A freshly installed tap is already enabled; do not issue a
                // redundant re-arm if its Started event arrives next tick.
                self.rearm_attempted = true;
                WakeRecoveryAction::Restart
            }
            State::Idle
            | State::WaitingForPermissions
            | State::Starting
            | State::Running
            | State::AlreadyRunning
            | State::Stopped
            | State::Failed(_) => WakeRecoveryAction::None,
        }
    }

    fn rearm_failed(&mut self) {
        // Most likely the tap thread cleared its registration immediately
        // before its Finished event reached the lifecycle channel. Retry the
        // lookup during this bounded window; once State becomes Stopped the
        // one permitted Restart action takes over.
        self.rearm_attempted = false;
    }
}

#[derive(Default)]
pub(super) struct TapRuntime {
    state: State,
    events: Option<mpsc::Receiver<Event>>,
    wake_recovery: WakeRecovery,
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

    pub(super) fn request_wake_recovery(&mut self) {
        self.wake_recovery.request(Instant::now());
    }

    pub(super) fn wake_recovery_pending(&self) -> bool {
        self.wake_recovery.deadline.is_some()
    }

    pub(super) fn recover_after_wake(
        &mut self,
        permissions_ready: bool,
        shared_config: &Arc<RwLock<AppConfig>>,
        runtime_control: &Arc<RuntimeControl>,
    ) {
        match self
            .wake_recovery
            .next_action(&self.state, permissions_ready, Instant::now())
        {
            WakeRecoveryAction::None => {}
            WakeRecoveryAction::Rearm => {
                if !event_tap::rearm_if_installed() {
                    self.wake_recovery.rearm_failed();
                }
            }
            WakeRecoveryAction::Restart => {
                self.spawn(Arc::clone(shared_config), Arc::clone(runtime_control))
            }
        }
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
        self.wake_recovery.cancel();
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
            wake_recovery: WakeRecovery::default(),
        };
        live.disabled();
        assert_eq!(live.state, State::Running);

        let mut starting = TapRuntime {
            state: State::Starting,
            events: None,
            wake_recovery: WakeRecovery::default(),
        };
        starting.disabled();
        assert_eq!(starting.state, State::Starting);

        let mut failed = TapRuntime {
            state: State::Failed("denied".to_string()),
            events: None,
            wake_recovery: WakeRecovery::default(),
        };
        failed.disabled();
        assert_eq!(failed.state, State::Idle);
    }

    #[test]
    fn wake_recovery_rearms_then_restarts_at_most_once_if_the_tap_stops() {
        let now = Instant::now();
        let mut recovery = WakeRecovery::default();
        recovery.request(now);

        assert_eq!(
            recovery.next_action(&State::Running, true, now),
            WakeRecoveryAction::Rearm
        );
        assert_eq!(
            recovery.next_action(&State::Running, true, now),
            WakeRecoveryAction::None
        );
        assert_eq!(
            recovery.next_action(&State::Stopped, true, now),
            WakeRecoveryAction::Restart
        );
        assert_eq!(
            recovery.next_action(&State::Failed("again".to_string()), true, now),
            WakeRecoveryAction::None
        );
    }

    #[test]
    fn wake_recovery_waits_for_permissions_but_expires() {
        let now = Instant::now();
        let mut recovery = WakeRecovery::default();
        recovery.request(now);

        assert_eq!(
            recovery.next_action(&State::Stopped, false, now),
            WakeRecoveryAction::None
        );
        assert_eq!(
            recovery.next_action(&State::Stopped, true, now + Duration::from_secs(1)),
            WakeRecoveryAction::Restart
        );

        let mut expired = WakeRecovery::default();
        expired.request(now);
        assert_eq!(
            expired.next_action(&State::Running, true, now + WAKE_RECOVERY_WINDOW),
            WakeRecoveryAction::None
        );
    }

    #[test]
    fn failed_port_lookup_can_be_retried_inside_the_recovery_window() {
        let now = Instant::now();
        let mut recovery = WakeRecovery::default();
        recovery.request(now);

        assert_eq!(
            recovery.next_action(&State::Running, true, now),
            WakeRecoveryAction::Rearm
        );
        recovery.rearm_failed();
        assert_eq!(
            recovery.next_action(&State::Running, true, now + Duration::from_millis(250)),
            WakeRecoveryAction::Rearm
        );
    }
}

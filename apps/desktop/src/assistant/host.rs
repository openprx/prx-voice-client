//! The assistant in the application: commands from the orb in, the loop's events out to the shared state, the session record to the History window, the microphone claim shared with call mode.
//!
//! The host owns at most one running session. Starting asks call mode for the microphone (refused
//! while a call runs), prepares and starts the session through a [`Launcher`] and reports every
//! typed problem as an issue; stopping asks the loop to end, and the host finishes the session
//! when the loop says it has ended: it saves the record, lets go of the microphone and, when
//! "End assistant and start" armed it, hands the microphone to call mode.

use prxvc_audio::call_pump::PumpEvent;
use prxvc_config::AppConfig;
use prxvc_desktop_ui::{AssistantIssue, EventSnapshot, HistoryEntry, SharedAppState};
use prxvc_session::assistant::{AssistantEvent, AssistantHandle, Phase, SessionRecorder};

use super::issues;
use super::launch::{Launched, Launcher};
use crate::call_compose::CallControl;

/// The state shown while the session's engines load (not a phase of the loop).
pub(crate) const STARTING: &str = "Starting";

/// The commands of the orb the assistant handles.
pub(crate) mod command {
    pub(crate) const CREATE: &str = "CreateSession";
    pub(crate) const START: &str = "StartSession";
    pub(crate) const STOP: &str = "StopSession";
    pub(crate) const INTERRUPT: &str = "InterruptPlayback";
}

struct Active {
    handle: AssistantHandle,
    recorder: Option<SessionRecorder>,
    devices: Option<prxvc_audio::call_pump::EventQueue>,
    on_end: Option<Box<dyn FnOnce() + Send>>,
    save_history: bool,
    save_transcript: bool,
    /// One line per turn and the summary: figures only.
    log: super::turn_log::TurnLog,
}

/// The assistant's side of the application.
pub(crate) struct AssistantHost {
    shared: SharedAppState,
    call: CallControl,
    storage: Option<prxvc_storage::StorageManager>,
    launcher: Box<dyn Launcher>,
    load_config: Box<dyn Fn() -> AppConfig + Send>,
    active: Option<Active>,
}

impl AssistantHost {
    /// A host that starts sessions with `launcher` on the configuration `load_config` gives.
    pub(crate) fn new(
        shared: SharedAppState,
        call: CallControl,
        storage: Option<prxvc_storage::StorageManager>,
        launcher: Box<dyn Launcher>,
        load_config: Box<dyn Fn() -> AppConfig + Send>,
    ) -> Self {
        Self {
            shared,
            call,
            storage,
            launcher,
            load_config,
            active: None,
        }
    }

    /// Whether a session is running.
    #[cfg(test)]
    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Apply a command of the orb; anything else is not the assistant's.
    pub(crate) fn command(&mut self, cmd: &str) {
        match cmd {
            command::CREATE => {}
            command::START => self.start(),
            command::STOP => self.stop(),
            command::INTERRUPT => {
                if let Some(active) = &self.active {
                    active.handle.interrupt();
                }
            }
            prxvc_desktop_ui::DISMISS_ISSUE => self.shared.set_assistant_issue(None),
            other => tracing::debug!(command = other, "not a command of the assistant"),
        }
    }

    fn start(&mut self) {
        if self.active.is_some() {
            return;
        }
        // An assistant session and call mode never share the microphone.
        if let Err(message) = self.call.claim_assistant() {
            self.show(issues::call_running(&message));
            return;
        }
        let config = (self.load_config)();
        // Loading the engines takes seconds: the orb and the pill say so from now on, and a click
        // while it goes on is ignored (never queued): the interface ignores it as well, and what
        // was queued before it could notice is dropped below.
        self.shared.update_session_state(STARTING, true);
        let launched = self.launcher.launch(&config);
        self.drop_clicks_during_start();
        match launched {
            Ok(launched) => self.begin(launched, &config),
            Err(found) => {
                self.shared
                    .update_session_state(Phase::Idle.ui_name(), false);
                for issue in &found {
                    tracing::warn!(key = %issue.key, detail = %issue.detail, "the assistant cannot start");
                }
                if let Some(first) = found.into_iter().next() {
                    self.show(first);
                }
                self.call.release_assistant();
            }
        }
    }

    /// Forget the start, stop and create commands that arrived while the session was starting: a
    /// second click is neither a stop nor a second start. Other commands are kept.
    fn drop_clicks_during_start(&self) {
        let mut dropped = 0usize;
        for cmd in self.shared.take_commands() {
            if matches!(
                cmd.as_str(),
                command::CREATE | command::START | command::STOP
            ) {
                dropped += 1;
            } else {
                self.shared.push_command(&cmd);
            }
        }
        if dropped > 0 {
            tracing::info!(
                dropped,
                "clicks while the assistant was starting were ignored"
            );
        }
    }

    fn begin(&mut self, launched: Launched, config: &AppConfig) {
        let Launched {
            handle,
            devices,
            on_end,
            language,
        } = launched;
        let recorder = SessionRecorder::new(
            &language,
            config.audio.auto_play_reply,
            config.privacy.save_history,
        )
        .map_err(|e| tracing::warn!("the session cannot be recorded: {e}"))
        .ok();
        let id = recorder.as_ref().map(|r| r.session().id.0.to_string());
        self.shared.set_assistant_issue(None);
        self.shared.clear_transcript();
        self.shared.set_session_id(id);
        self.shared
            .update_session_state(Phase::Listening.ui_name(), true);
        self.active = Some(Active {
            handle,
            recorder,
            devices,
            on_end,
            save_history: config.privacy.save_history,
            save_transcript: config.privacy.save_transcript,
            log: super::turn_log::TurnLog::default(),
        });
    }

    fn stop(&mut self) {
        if let Some(active) = &self.active {
            active.handle.stop();
        }
    }

    fn show(&self, issue: AssistantIssue) {
        self.shared.push_event(EventSnapshot {
            event_type: "AssistantIssue".into(),
            detail: format!("{}: {}", issue.key, issue.detail),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
        self.shared.set_assistant_issue(Some(issue));
    }

    /// Take what the loop reported and the housekeeping of every turn of the host's loop.
    pub(crate) fn tick(&mut self) {
        super::launch::expire_warm();
        let mut ended = false;
        let mut events = Vec::new();
        if let Some(active) = &self.active {
            while let Some(e) = active.handle.try_event() {
                events.push(e);
            }
            if let Some(queue) = &active.devices {
                for e in queue.drain() {
                    match e {
                        PumpEvent::DeviceChanged { device } => {
                            self.shared.set_input_device(&device)
                        }
                        PumpEvent::DeviceUnavailable { .. } => self.shared.set_input_device(""),
                        _ => {}
                    }
                }
            }
        }
        for event in events {
            ended |= self.on_event(event);
        }
        if ended {
            self.finish();
        }
        if self.active.is_none() {
            // Give the microphone back to call mode, and send the start "End assistant and start"
            // armed, once nothing of the assistant runs.
            self.call.release_assistant();
        }
    }

    /// Show one event of the loop; true when it was the end of the session.
    fn on_event(&mut self, event: AssistantEvent) -> bool {
        if let Some(active) = self.active.as_mut()
            && let Some(line) = active.log.on_event(std::time::Instant::now(), &event)
        {
            tracing::info!("assistant {line}");
        }
        if let Some(recorder) = self.active.as_mut().and_then(|a| a.recorder.as_mut()) {
            recorder.on_event(&event);
        }
        let shared = &self.shared;
        match event {
            AssistantEvent::Started { device } => shared.set_input_device(&device),
            AssistantEvent::Phase(phase) => {
                shared.update_session_state(phase.ui_name(), phase != Phase::Idle);
                if phase == Phase::Listening {
                    // Speech that never became a turn (a hum, filler) leaves no line behind.
                    shared.drop_interim_user_line();
                }
            }
            AssistantEvent::Partial { text } => shared.set_interim_user_line(&text),
            AssistantEvent::UserTurn { text } => {
                shared.finish_user_line(&text);
                shared.set_assistant_issue(None);
            }
            AssistantEvent::ReplyDelta { text } => shared.append_assistant_text(&text),
            AssistantEvent::ReplyDone | AssistantEvent::ReplyCut => shared.finish_assistant_line(),
            AssistantEvent::MicPaused(paused) => shared.set_mic_paused(paused),
            AssistantEvent::Fault(fault) => self.show(issues::from_fault(&fault)),
            AssistantEvent::Ended => return true,
        }
        false
    }

    /// The loop has ended: save the record, show the idle state, let go of the session.
    fn finish(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };
        let Active {
            handle,
            recorder,
            on_end,
            save_history,
            save_transcript,
            log,
            ..
        } = active;
        handle.join(|_| {});
        tracing::info!("{}", log.summary());
        if let Some(recorder) = recorder {
            let session = recorder.finish();
            if save_history {
                self.persist(&session, save_transcript);
            }
        }
        self.shared.clear_transcript();
        self.shared.set_mic_paused(false);
        self.shared.set_input_device("");
        self.shared.set_session_id(None);
        self.shared
            .update_session_state(Phase::Idle.ui_name(), false);
        if let Some(on_end) = on_end {
            on_end();
        }
    }

    fn persist(&self, session: &prxvc_session::Session, with_preview: bool) {
        let Some(storage) = &self.storage else {
            return;
        };
        // The first thing the user said is the title of the entry, when transcripts may be kept.
        let preview = with_preview
            .then(|| {
                session
                    .turns
                    .iter()
                    .find_map(|t| t.user_transcript.clone())
                    .map(|text| text.chars().take(80).collect::<String>())
            })
            .flatten();
        let mut record = match prxvc_storage::SessionRecord::from_session(session) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("failed to describe the assistant session: {e}");
                return;
            }
        };
        record.transcript_preview = preview.clone();
        match storage.save_session(session, preview) {
            Ok(()) => {
                tracing::info!("assistant session persisted: {}", session.id.0);
                self.shared.add_history_entry(HistoryEntry {
                    session_id: record.id.clone(),
                    title: record.summary_title(),
                    created_at: record.created_at.clone(),
                    duration: record.duration_display(),
                    state: record.state.clone(),
                });
            }
            Err(e) => tracing::warn!("failed to persist the assistant session: {e}"),
        }
    }
}

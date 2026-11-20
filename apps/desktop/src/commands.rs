//! UI command dispatch: maps command strings from the UI to engine commands and runs the follow-up work.

use prxvc_audio::audio_loop::{AudioProcessingLoop, LoopConfig};
use prxvc_core::engine::Engine;
use prxvc_desktop_ui::{NetworkStatus, SharedAppState};
use prxvc_protocol::Command;

use crate::asr_session::{AudioWiring, start_audio_loop};
use crate::diagnostics::export_diagnostics;
use crate::event_map::push_error;
use crate::persistence::persist_session;
use crate::playback;

/// The engine thread's collaborators that command handling reads or changes.
pub(crate) struct CommandContext<'a> {
    pub engine: &'a mut Engine,
    pub shared: &'a SharedAppState,
    pub storage: &'a Option<prxvc_storage::StorageManager>,
    pub audio_loop: &'a mut AudioProcessingLoop,
    pub audio_wiring: &'a AudioWiring,
    pub playback: &'a mut playback::PlaybackDriver,
    pub loop_config: LoopConfig,
}

/// Map a UI command string to the appropriate [`Command`] and dispatch it
/// through the engine. Handles session creation/start chaining and
/// post-stop persistence.
pub(crate) fn process_command(ctx: &mut CommandContext<'_>, cmd: &str) {
    tracing::info!("process_command: {}", cmd);

    let Some(command) = resolve_command(ctx, cmd) else {
        return;
    };

    // Capture session id before the command mutates state (for StopSession).
    let is_stop = cmd == "StopSession";
    let pre_session_id = ctx.engine.active_session().map(|s| s.id);

    // Stop audio capture before StopSession.
    if is_stop && ctx.audio_loop.is_running() {
        ctx.audio_loop.stop();
    }

    let result = ctx.engine.process_command(command);

    // Interrupt and stop silence the assistant locally whatever the engine
    // decided: it may consider the reply finished while queued audio plays on.
    if cmd == "InterruptPlayback" || is_stop {
        ctx.playback.flush(ctx.engine, std::time::Instant::now());
    }

    match result {
        Ok(()) => after_command(ctx, cmd, pre_session_id),
        Err(e) => {
            push_error(ctx.shared, &format!("Command '{cmd}' failed: {e}"));
        }
    }
}

/// Turn a command string into an engine [`Command`]. Returns `None` when the
/// command was handled locally (retry, export, ignored) or could not be built.
fn resolve_command(ctx: &mut CommandContext<'_>, cmd: &str) -> Option<Command> {
    let engine = &mut *ctx.engine;
    let shared = ctx.shared;
    let command = match cmd {
        "CreateSession" => Command::CreateSession,
        "StartSession" => {
            // If there is already an active session, start it.
            // Otherwise create one first, then start it.
            if let Some(session) = engine.active_session() {
                Command::StartSession {
                    session_id: session.id.0,
                }
            } else {
                if let Err(e) = engine.process_command(Command::CreateSession) {
                    push_error(shared, &format!("Create session: {e}"));
                    return None;
                }
                if let Some(session) = engine.active_session() {
                    let sid = session.id.0;
                    shared.set_session_id(Some(sid.to_string()));
                    Command::StartSession { session_id: sid }
                } else {
                    push_error(shared, "No active session after create");
                    return None;
                }
            }
        }
        "StopSession" => {
            if let Some(session) = engine.active_session() {
                Command::StopSession {
                    session_id: session.id.0,
                }
            } else {
                tracing::debug!("StopSession ignored: no active session");
                return None;
            }
        }
        "InterruptPlayback" => {
            if let Some(session) = engine.active_session() {
                Command::InterruptPlayback {
                    session_id: session.id.0,
                }
            } else {
                tracing::debug!("InterruptPlayback: no active session, flushing local playback");
                ctx.playback.flush(engine, std::time::Instant::now());
                return None;
            }
        }
        "RefreshDeviceList" => Command::RefreshDeviceList,
        "RetryConnection" => {
            shared.clear_error();
            shared.set_network_status(NetworkStatus::Reconnecting);
            // Re-initialize the engine transport to attempt reconnection.
            if let Err(e) = engine.initialize() {
                shared.set_error(&format!("Reconnect failed: {e}"), true);
                shared.set_network_status(NetworkStatus::Failed);
            } else {
                shared.set_network_status(NetworkStatus::Connected);
            }
            return None;
        }
        "ExportDiagnostics" => {
            export_diagnostics(shared);
            return None;
        }
        _ => {
            tracing::debug!("unknown command: {cmd}");
            return None;
        }
    };
    Some(command)
}

/// Work that follows a command the engine accepted: propagate the session id,
/// start the ASR loop after a start, persist and reset after a stop.
fn after_command(
    ctx: &mut CommandContext<'_>,
    cmd: &str,
    pre_session_id: Option<prxvc_session::SessionId>,
) {
    let is_stop = cmd == "StopSession";

    // After CreateSession, propagate the session id to the UI.
    if cmd == "CreateSession" {
        if let Some(session) = ctx.engine.active_session() {
            ctx.shared.set_session_id(Some(session.id.0.to_string()));
        }
    }

    // After StartSession, start the local audio ASR loop.
    if cmd == "StartSession" {
        start_audio_loop(
            ctx.engine,
            ctx.audio_loop,
            ctx.audio_wiring,
            ctx.shared,
            ctx.playback.gate(),
            ctx.loop_config,
        );
    }

    // After StopSession, persist the completed session and reset UI.
    if is_stop {
        if let (Some(storage), Some(session_id)) = (ctx.storage, pre_session_id) {
            persist_session(ctx.engine, &session_id, storage, ctx.shared);
        }
        ctx.shared.clear_transcript();
        ctx.shared.update_session_state("Idle", false);
        ctx.shared.set_session_id(None);
    }
}

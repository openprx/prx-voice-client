//! The loop of the assistant's host thread: UI commands in, the session's events out, every 30 ms.

use std::time::Duration;

use prxvc_desktop_ui::SharedAppState;

use super::host::AssistantHost;
use super::launch::RealLauncher;
use crate::call_compose::CallControl;
use crate::diagnostics::export_diagnostics;

/// How often the host looks at the commands and the events of the session.
const TICK: Duration = Duration::from_millis(30);

/// Handle the commands that are not the assistant's own: call mode, diagnostics and the driver.
/// Returns whether `cmd` was one of them.
fn other_command(shared: &SharedAppState, call: &CallControl, cmd: &str) -> bool {
    if call.handle_command(cmd) {
        return true;
    }
    match cmd {
        "ExportDiagnostics" => export_diagnostics(shared),
        "RefreshDriverStatus" | "InstallDriver" | "UninstallDriver" => {
            crate::driver_install::handle_command(shared, cmd);
        }
        _ => return false,
    }
    true
}

/// Apply one command of the UI.
pub(crate) fn pump_one(
    host: &mut AssistantHost,
    shared: &SharedAppState,
    call: &CallControl,
    cmd: &str,
) {
    // "End assistant and start" ends the assistant session like the end button does; the call
    // starts when the session has let go of the microphone (`AssistantHost::tick`).
    if cmd == prxvc_desktop_ui::call_view::command::END_ASSISTANT_AND_START {
        call.arm_start_after_assistant();
        host.command(super::host::command::STOP);
    } else if !other_command(shared, call, cmd) {
        host.command(cmd);
    }
}

/// One turn of the host: take the commands, then the events.
pub(crate) fn pump(host: &mut AssistantHost, shared: &SharedAppState, call: &CallControl) {
    for cmd in shared.take_commands() {
        pump_one(host, shared, call, &cmd);
    }
    host.tick();
}

/// Run the host on its own thread until the application ends.
pub(crate) fn spawn(
    shared: SharedAppState,
    call: CallControl,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("prxvc-assistant-host".into())
        .spawn(move || {
            let storage = crate::compose::initialize_storage();
            if let Some(storage) = &storage {
                crate::persistence::load_history_from_storage(storage, &shared);
            }
            let mut host = AssistantHost::new(
                shared.clone(),
                call.clone(),
                storage,
                Box::new(RealLauncher),
                Box::new(crate::call_kit::config::load_current),
            );
            loop {
                pump(&mut host, &shared, &call);
                std::thread::sleep(TICK);
            }
        })
}

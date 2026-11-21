//! The app's side of the settings window: saving its edits, running its commands, remembering its place, and keeping its data fresh.
//!
//! The window edits [`UiSettings`] and queues [`SettingsCommand`]s; this module turns them into
//! effects, off the UI thread wherever they can block:
//!
//! * **Edits** are diffed against the last stored state and written through the configuration's
//!   atomic `update` by a writer thread, one field at a time (`settings::apply_changes`). A write
//!   that fails puts the old value back and shows a notice.
//! * **Commands** open System Settings panes and folders, ask for permissions, start the driver
//!   operations of `driver_install`, and hand `ExportDiagnostics` to the
//!   host's command queue.
//! * **Data** (devices, permissions, the driver, the pipeline) is probed by a thread of its own
//!   when the window opens, regains focus, or the user asks, and published through the feed,
//!   which wakes the UI.
//! * **Memory**: geometry and page are stored in `ui.settings_window` when the window closes and
//!   when the app quits; the value the old build kept in eframe's storage is migrated once.
//!
//! The host is created on the first frame (it needs the egui context and the shared state) and
//! lives for the process, so that the composition root does not have to know about it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use prxvc_config::{AppConfig, AppConfigManager, LoadNotice, SettingsWindowConfig};
use prxvc_desktop_ui::settings_data::{DriverOperationView, SettingsData, SettingsNotice};
use prxvc_desktop_ui::{
    DesktopUiState, RevealTarget, SettingsCommand, SettingsFeed, SharedAppState, SystemPane,
    UiSettings,
};

use crate::settings::{self, SettingsSnapshot};

/// How often the driver status is read while an install or uninstall runs.
const DRIVER_POLL: Duration = Duration::from_millis(500);

/// A job for the writer thread.
enum WriteJob {
    /// Write the fields that differ between `before` and `after`.
    Settings {
        before: Box<UiSettings>,
        after: Box<UiSettings>,
    },
    /// Store the settings window's place.
    Window(SettingsWindowConfig),
    /// Acknowledge once every earlier job is done.
    Flush(Sender<()>),
}

/// What a write job reports back to the UI thread.
enum WriteResult {
    /// Stored.
    Saved,
    /// Not stored; the old values must come back.
    SettingsFailed {
        before: Box<UiSettings>,
        after: Box<UiSettings>,
        detail: String,
    },
    /// The window's place could not be stored.
    WindowFailed(String),
}

/// A job for the probe thread.
enum ProbeJob {
    /// Read everything again.
    Refresh,
}

struct Host {
    shared: SharedAppState,
    feed: SettingsFeed,
    probes: Sender<ProbeJob>,
    writes: Sender<WriteJob>,
    results: Receiver<WriteResult>,
    /// The start-up configuration has been applied.
    started: bool,
    /// The settings window was open in the previous frame.
    was_open: bool,
    /// The settings window was opened during this run (its place is worth storing).
    touched: bool,
}

static HOST: OnceLock<Mutex<Host>> = OnceLock::new();

fn with_host<R>(
    ctx: &egui::Context,
    ui_state: &mut DesktopUiState,
    shared: &SharedAppState,
    f: impl FnOnce(&mut Host, &mut DesktopUiState) -> R,
) -> R {
    let host = HOST.get_or_init(|| Mutex::new(Host::start(ctx, ui_state, shared)));
    let mut guard = host.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard, ui_state)
}

impl Host {
    /// Create the feed, the writer thread and the probe thread.
    fn start(ctx: &egui::Context, ui_state: &mut DesktopUiState, shared: &SharedAppState) -> Self {
        ui_state
            .settings_feed
            .set_wake(shared.repaint_signal().clone());
        let feed = ui_state.settings_feed.clone();
        let (writes, write_rx) = channel();
        let (result_tx, results) = channel();
        let wake = ctx.clone();
        std::thread::Builder::new()
            .name("settings-writer".into())
            .spawn(move || write_loop(&write_rx, &result_tx, &wake))
            .map_err(|e| tracing::error!("settings writer thread: {e}"))
            .ok();
        let (probes, probe_rx) = channel();
        let probe_feed = feed.clone();
        std::thread::Builder::new()
            .name("settings-probe".into())
            .spawn(move || probe_loop(&probe_rx, &probe_feed))
            .map_err(|e| tracing::error!("settings probe thread: {e}"))
            .ok();
        Self {
            shared: shared.clone(),
            feed,
            probes,
            writes,
            results,
            started: false,
            was_open: false,
            touched: false,
        }
    }

    /// Apply what the configuration says to the running app, once.
    fn apply_startup(
        &mut self,
        ctx: &egui::Context,
        ui_state: &mut DesktopUiState,
        config: &AppConfig,
    ) {
        self.started = true;
        // Interface language: "system" follows the operating system.
        let locale =
            settings::resolve_locale(&config.ui.language, sys_locale::get_locale().as_deref());
        prxvc_i18n::set_locale(locale);
        // Theme.
        if ui_state.settings.theme != prxvc_desktop_ui::theme::RF_THEME_NAME {
            ui_state.pending_theme = Some(ui_state.settings.theme.clone());
        }
        // The orb window level (the window is created always-on-top).
        if !config.ui.always_on_top {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
        }
        // The settings window's place and page.
        let saved = settings::window_from_config(&config.ui.settings_window);
        if let Some(section) = saved.section {
            ui_state.settings_view.section = section;
        }
        match saved.geometry {
            Some((geometry, monitor)) => {
                ui_state
                    .windows
                    .settings
                    .restore_on_monitor(geometry, monitor);
            }
            None => {
                // The previous build kept the place in eframe's storage; the composition root has
                // restored it into the window state. Store it in the configuration, once.
                if let Some(geometry) = ui_state.windows.settings.geometry() {
                    let window = settings::window_to_config(
                        Some(geometry),
                        None,
                        ui_state.settings_view.section,
                    );
                    let _ = self.writes.send(WriteJob::Window(window));
                }
            }
        }
        // Notices of the settings file load.
        let kept: Vec<String> = crate::config_load::load_notices()
            .iter()
            .filter_map(|n| match n {
                LoadNotice::UnreadableFileKept { kept_as, .. } => {
                    Some(kept_as.display().to_string())
                }
                _ => None,
            })
            .collect();
        self.feed.update(|d| {
            for kept_as in kept {
                d.notices
                    .push(SettingsNotice::UnreadableFileKept { kept_as });
            }
        });
    }

    fn refresh(&self) {
        let _ = self.probes.send(ProbeJob::Refresh);
    }

    /// Take what the writer reports.
    fn collect_results(
        &mut self,
        ui_state: &mut DesktopUiState,
        last_saved: &mut SettingsSnapshot,
    ) {
        while let Ok(result) = self.results.try_recv() {
            match result {
                WriteResult::Saved => self.feed.update(|d| {
                    d.notices
                        .retain(|n| !matches!(n, SettingsNotice::SaveFailed { .. }));
                }),
                WriteResult::SettingsFailed {
                    before,
                    after,
                    detail,
                } => {
                    tracing::warn!("settings not saved: {detail}");
                    settings::revert_changes(&mut ui_state.settings, &before, &after);
                    settings::revert_changes(last_saved, &before, &after);
                    self.feed.update(|d| {
                        d.notices
                            .retain(|n| !matches!(n, SettingsNotice::SaveFailed { .. }));
                        d.notices.push(SettingsNotice::SaveFailed { detail });
                    });
                }
                WriteResult::WindowFailed(detail) => {
                    tracing::warn!("settings window place not saved: {detail}");
                }
            }
        }
    }

    /// Store the edits made since the last frame and apply their immediate effects.
    fn save_edits(
        &mut self,
        ctx: &egui::Context,
        ui_state: &DesktopUiState,
        last_saved: &mut SettingsSnapshot,
    ) {
        if ui_state.settings == *last_saved {
            return;
        }
        let (before, after) = (last_saved.clone(), ui_state.settings.clone());
        if before.language != after.language {
            let locale =
                settings::resolve_locale(&after.language, sys_locale::get_locale().as_deref());
            prxvc_i18n::set_locale(locale);
            // Sentences the probes produced are in the old language.
            self.refresh();
        }
        if before.always_on_top != after.always_on_top {
            let level = if after.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
        }
        if before.offline != after.offline || before.allow_hosts != after.allow_hosts {
            // The providers' verdicts depend on the offline policy.
            self.refresh();
        }
        *last_saved = after.clone();
        let _ = self.writes.send(WriteJob::Settings {
            before: Box::new(before),
            after: Box::new(after),
        });
    }

    /// Store the settings window's place and page.
    fn remember_window(&self, ui_state: &DesktopUiState) {
        let window = settings::window_to_config(
            ui_state.windows.settings.geometry(),
            ui_state.windows.settings.monitor(),
            ui_state.settings_view.section,
        );
        let _ = self.writes.send(WriteJob::Window(window));
    }

    fn run_command(&self, command: SettingsCommand) {
        match command {
            SettingsCommand::Refresh => self.refresh(),
            SettingsCommand::RequestMicrophone => {
                self.request(crate::settings_data::request_microphone, true)
            }
            SettingsCommand::RequestCallAudio => {
                self.request(crate::settings_data::request_call_audio, false)
            }
            SettingsCommand::OpenSystemPane(pane) => open_system_pane(pane),
            SettingsCommand::InstallDriver => {
                crate::driver_install::handle_command(&self.shared, "InstallDriver");
                self.refresh();
            }
            SettingsCommand::UninstallDriver => {
                crate::driver_install::handle_command(&self.shared, "UninstallDriver");
                self.refresh();
            }
            SettingsCommand::Reveal(target) => {
                if let Some(path) = reveal_path(target, &self.feed.snapshot()) {
                    reveal(&path);
                }
            }
            SettingsCommand::RetryModelServer => {
                if let Some(server) = crate::llm_server::global() {
                    server.retry();
                }
                self.refresh();
            }
            SettingsCommand::ExportDiagnostics => self.shared.push_command("ExportDiagnostics"),
        }
    }

    /// Run a permission request (which blocks until the user answers) on a thread of its own,
    /// then record its outcome and read the status again.
    fn request(
        &self,
        ask: fn() -> Option<prxvc_desktop_ui::settings_data::RequestOutcome>,
        microphone: bool,
    ) {
        let feed = self.feed.clone();
        let probes = self.probes.clone();
        let spawned = std::thread::Builder::new()
            .name("permission-request".into())
            .spawn(move || {
                let outcome = ask();
                feed.update(|d| {
                    if microphone {
                        d.permissions.microphone_request = outcome;
                    } else {
                        d.permissions.call_audio_request = outcome;
                    }
                });
                let _ = probes.send(ProbeJob::Refresh);
            });
        if let Err(e) = spawned {
            tracing::error!("permission request thread: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// The per-frame entry points
// ---------------------------------------------------------------------------

/// Before drawing: set up on the first frame, take the writer's results and the probed data.
pub(crate) fn before_frame(
    ctx: &egui::Context,
    ui_state: &mut DesktopUiState,
    shared: &SharedAppState,
    config: &AppConfig,
    last_saved: &mut SettingsSnapshot,
) {
    with_host(ctx, ui_state, shared, |host, ui_state| {
        if !host.started {
            host.apply_startup(ctx, ui_state, config);
        }
        host.collect_results(ui_state, last_saved);
    });
    ui_state.sync_settings_feed();
}

/// After drawing: store edits, run commands, remember the window when it closed.
pub(crate) fn after_frame(
    ctx: &egui::Context,
    ui_state: &mut DesktopUiState,
    shared: &SharedAppState,
    last_saved: &mut SettingsSnapshot,
) {
    with_host(ctx, ui_state, shared, |host, ui_state| {
        host.save_edits(ctx, ui_state, last_saved);
        let open = ui_state.windows.settings.is_open();
        host.touched |= open;
        if host.was_open && !open {
            host.remember_window(ui_state);
        }
        host.was_open = open;
        for command in std::mem::take(&mut ui_state.settings_commands) {
            host.run_command(command);
        }
    });
}

/// When the app quits: store the window's place (if it was used) and wait for the writer.
pub(crate) fn on_exit(ui_state: &DesktopUiState) {
    let Some(host) = HOST.get() else { return };
    let host = host.lock().unwrap_or_else(|e| e.into_inner());
    if host.touched {
        host.remember_window(ui_state);
    }
    let (ack, done) = channel();
    if host.writes.send(WriteJob::Flush(ack)).is_ok() {
        let _ = done.recv_timeout(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------------
// Threads
// ---------------------------------------------------------------------------

/// Run write jobs in order, report failures to the UI thread.
fn write_loop(jobs: &Receiver<WriteJob>, results: &Sender<WriteResult>, wake: &egui::Context) {
    let manager = AppConfigManager::with_default_dir();
    while let Ok(job) = jobs.recv() {
        match job {
            WriteJob::Flush(ack) => {
                let _ = ack.send(());
            }
            job => {
                let result = match &manager {
                    Ok(manager) => run_write(manager, job),
                    Err(e) => fail(job, e.to_string()),
                };
                let _ = results.send(result);
                wake.request_repaint();
            }
        }
    }
}

/// Run one job against `manager` (pure of threads, for the tests).
fn run_write(manager: &AppConfigManager, job: WriteJob) -> WriteResult {
    match job {
        WriteJob::Settings { before, after } => {
            match manager.update(|c| settings::apply_changes(c, &before, &after)) {
                Ok(_) => {
                    // Selecting the language model starts the model server, selecting system
                    // translation stops it.
                    if before.call_translation_engine != after.call_translation_engine {
                        crate::llm_server::on_engine_changed(&after.call_translation_engine);
                    }
                    WriteResult::Saved
                }
                Err(e) => WriteResult::SettingsFailed {
                    before,
                    after,
                    detail: e.to_string(),
                },
            }
        }
        WriteJob::Window(window) => match manager.update(|c| c.ui.settings_window = window) {
            Ok(_) => WriteResult::Saved,
            Err(e) => WriteResult::WindowFailed(e.to_string()),
        },
        WriteJob::Flush(_) => WriteResult::Saved,
    }
}

fn fail(job: WriteJob, detail: String) -> WriteResult {
    match job {
        WriteJob::Settings { before, after } => WriteResult::SettingsFailed {
            before,
            after,
            detail,
        },
        WriteJob::Window(_) | WriteJob::Flush(_) => WriteResult::WindowFailed(detail),
    }
}

/// Probe on request, and every [`DRIVER_POLL`] while a driver operation runs.
fn probe_loop(jobs: &Receiver<ProbeJob>, feed: &SettingsFeed) {
    let mut driver_working = false;
    loop {
        let request = if driver_working {
            match jobs.recv_timeout(DRIVER_POLL) {
                Ok(job) => Some(job),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Some(ProbeJob::Refresh),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => None,
            }
        } else {
            jobs.recv().ok()
        };
        if request.is_none() {
            return;
        }
        // Requests that piled up while probing are answered by this probe.
        while jobs.try_recv().is_ok() {}
        let config = crate::call_kit::config::load_current();
        let previous = feed.snapshot();
        let data = crate::settings_data::collect(&config, &previous);
        // The driver operation and the model server are polled while they change.
        driver_working = matches!(
            data.permissions.driver.operation,
            DriverOperationView::Working(_)
        ) || crate::settings_data::engine_in_motion(&config);
        feed.update(|d| {
            let notices = std::mem::take(&mut d.notices);
            *d = data;
            d.notices = notices;
        });
    }
}

// ---------------------------------------------------------------------------
// Opening things
// ---------------------------------------------------------------------------

/// The file or folder `target` names, from the published facts.
fn reveal_path(target: RevealTarget, data: &SettingsData) -> Option<PathBuf> {
    let about = &data.about;
    let text = match target {
        RevealTarget::DataDir => Some(about.data_dir.clone()),
        RevealTarget::ModelsDir => Some(about.models_dir.clone()),
        RevealTarget::LogFile => about.log_file.clone(),
        RevealTarget::ConfigFile => Some(about.config_file.clone()),
        RevealTarget::Notices => about.notices_file.clone(),
    };
    text.filter(|t| !t.is_empty()).map(PathBuf::from)
}

/// The program and arguments that show `path` in the system's file manager on `os`.
fn reveal_command(path: &Path, os: &str) -> Option<(&'static str, Vec<String>)> {
    let shown = path.display().to_string();
    match os {
        "macos" if path.is_dir() => Some(("open", vec![shown])),
        "macos" => Some(("open", vec!["-R".into(), shown])),
        "windows" => Some(("explorer", vec![format!("/select,{shown}")])),
        "linux" => {
            let folder = if path.is_dir() { path } else { path.parent()? };
            Some(("xdg-open", vec![folder.display().to_string()]))
        }
        _ => None,
    }
}

/// The program and arguments that open `url` on `os`.
fn open_url_command(url: &str, os: &str) -> Option<(&'static str, Vec<String>)> {
    match os {
        "macos" => Some(("open", vec![url.to_owned()])),
        "windows" => Some((
            "cmd",
            vec!["/C".into(), "start".into(), String::new(), url.to_owned()],
        )),
        "linux" => Some(("xdg-open", vec![url.to_owned()])),
        _ => None,
    }
}

fn run_detached(command: Option<(&'static str, Vec<String>)>) {
    let Some((program, args)) = command else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("open".into())
        .spawn(
            move || match std::process::Command::new(program).args(&args).status() {
                Ok(status) if status.success() => {}
                Ok(status) => tracing::warn!("{program} exited with {status}"),
                Err(e) => tracing::warn!("could not run {program}: {e}"),
            },
        );
    if let Err(e) = spawned {
        tracing::warn!("could not start a thread to open a file or pane: {e}");
    }
}

/// Open a System Settings pane (also used by the Call translation window's actions).
pub(crate) fn open_pane(pane: SystemPane) {
    open_system_pane(pane);
}

fn open_system_pane(pane: SystemPane) {
    let Some(url) = crate::settings_data::system_pane_url(pane, std::env::consts::OS) else {
        tracing::warn!("no System Settings pane for {pane:?} on this platform");
        return;
    };
    run_detached(open_url_command(url, std::env::consts::OS));
}

fn reveal(path: &Path) {
    run_detached(reveal_command(path, std::env::consts::OS));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(dir: &Path) -> AppConfigManager {
        AppConfigManager::with_dir(dir.to_path_buf())
    }

    fn settings_job(change: impl FnOnce(&mut UiSettings)) -> (WriteJob, UiSettings) {
        let before = settings::ui_settings_from_config(&AppConfig::default());
        let mut after = before.clone();
        change(&mut after);
        (
            WriteJob::Settings {
                before: Box::new(before),
                after: Box::new(after.clone()),
            },
            after,
        )
    }

    #[test]
    fn a_settings_write_changes_only_its_key_in_the_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let m = manager(dir.path());
        // The file has a value the window knows nothing about, edited by hand.
        m.update(|c| c.assistant.history_max_turns = 9)
            .expect("seed");
        let (job, _) = settings_job(|s| s.call_local_output_mode = "both".into());
        assert!(matches!(run_write(&m, job), WriteResult::Saved));
        let read = m.load().expect("load");
        assert_eq!(read.call.local_output_mode, "both");
        assert_eq!(
            read.assistant.history_max_turns, 9,
            "the other key is untouched"
        );
        // Exactly the changed key differs from the defaults and the seed.
        let mut expected = AppConfig::default();
        expected.assistant.history_max_turns = 9;
        expected.call.local_output_mode = "both".into();
        assert_eq!(read, expected);
    }

    #[test]
    fn two_writes_of_different_fields_both_persist() {
        let dir = tempfile::tempdir().expect("tmp");
        let m = manager(dir.path());
        let (a, _) = settings_job(|s| s.offline = true);
        let (b, _) = settings_job(|s| s.echo_mode = "off".into());
        assert!(matches!(run_write(&m, a), WriteResult::Saved));
        assert!(matches!(run_write(&m, b), WriteResult::Saved));
        let read = m.load().expect("load");
        assert!(read.network_policy.offline);
        assert_eq!(read.audio.echo_mode, prxvc_config::EchoMode::Off);
        assert!(!read.audio.echo_control, "the legacy flag follows");
    }

    #[test]
    fn the_window_place_is_stored_under_ui_settings_window() {
        let dir = tempfile::tempdir().expect("tmp");
        let m = manager(dir.path());
        let geometry = prxvc_desktop_ui::tool_window::Geometry {
            pos: egui::pos2(12.0, 34.0),
            size: egui::vec2(700.0, 640.0),
        };
        let window = settings::window_to_config(
            Some(geometry),
            Some(egui::vec2(1440.0, 900.0)),
            prxvc_desktop_ui::SettingsSection::Audio,
        );
        assert!(matches!(
            run_write(&m, WriteJob::Window(window)),
            WriteResult::Saved
        ));
        let read = m.load().expect("load");
        let saved = settings::window_from_config(&read.ui.settings_window);
        assert_eq!(
            saved.geometry,
            Some((geometry, Some(egui::vec2(1440.0, 900.0))))
        );
        assert_eq!(
            saved.section,
            Some(prxvc_desktop_ui::SettingsSection::Audio)
        );
        assert_eq!(
            read.ui.language,
            AppConfig::default().ui.language,
            "other keys are untouched"
        );
    }

    #[test]
    fn a_write_that_cannot_complete_reports_the_values_to_put_back() {
        // The configuration directory is a file, so nothing can be written.
        let dir = tempfile::tempdir().expect("tmp");
        let blocker = dir.path().join("blocked");
        std::fs::write(&blocker, "x").expect("file");
        let m = manager(&blocker);
        let (job, after) = settings_job(|s| s.auto_play = false);
        match run_write(&m, job) {
            WriteResult::SettingsFailed {
                before,
                after: failed_after,
                detail,
            } => {
                assert!(before.auto_play);
                assert_eq!(*failed_after, after);
                assert!(!detail.is_empty());
            }
            _ => panic!("the write must fail"),
        }
        // The same failure for a manager that could not be created at all.
        let (job, _) = settings_job(|s| s.auto_play = false);
        assert!(matches!(
            fail(job, "no dir".into()),
            WriteResult::SettingsFailed { .. }
        ));
        assert!(matches!(
            fail(
                WriteJob::Window(SettingsWindowConfig::default()),
                "no dir".into()
            ),
            WriteResult::WindowFailed(_)
        ));
    }

    #[test]
    fn folders_and_files_are_revealed_the_way_each_system_does_it() {
        let dir = tempfile::tempdir().expect("tmp");
        let file = dir.path().join("config.toml");
        std::fs::write(&file, "x").expect("file");
        assert_eq!(
            reveal_command(&file, "macos").map(|c| c.1[0].clone()),
            Some("-R".to_owned())
        );
        assert_eq!(
            reveal_command(dir.path(), "macos").map(|c| c.1.len()),
            Some(1),
            "a folder is opened"
        );
        assert_eq!(
            reveal_command(&file, "windows").map(|c| c.0),
            Some("explorer")
        );
        assert_eq!(
            reveal_command(&file, "linux").map(|c| c.1),
            Some(vec![dir.path().display().to_string()])
        );
        assert_eq!(reveal_command(&file, "plan9"), None);
    }

    #[test]
    fn urls_are_opened_with_the_system_opener() {
        assert_eq!(
            open_url_command("x-apple.systempreferences:a", "macos").map(|c| c.0),
            Some("open")
        );
        assert_eq!(
            open_url_command("ms-settings:privacy-microphone", "windows").map(|c| c.0),
            Some("cmd")
        );
        assert_eq!(
            open_url_command("u", "linux").map(|c| c.0),
            Some("xdg-open")
        );
        assert_eq!(open_url_command("u", "plan9"), None);
    }

    #[test]
    fn reveal_targets_come_from_the_published_facts() {
        let mut data = SettingsData::default();
        data.about.data_dir = "/data".into();
        data.about.config_file = "/data/config.toml".into();
        data.about.log_file = Some("/logs/prx-voice.log".into());
        assert_eq!(
            reveal_path(RevealTarget::DataDir, &data),
            Some(PathBuf::from("/data"))
        );
        assert_eq!(
            reveal_path(RevealTarget::LogFile, &data),
            Some(PathBuf::from("/logs/prx-voice.log"))
        );
        assert_eq!(
            reveal_path(RevealTarget::Notices, &data),
            None,
            "no notices file is known"
        );
        assert_eq!(
            reveal_path(RevealTarget::ModelsDir, &data),
            None,
            "an empty path is nothing"
        );
    }
}

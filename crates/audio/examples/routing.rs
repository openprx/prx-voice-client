//! Call-mode routing from the command line: `status` is read-only; `enter`, `leave` and `recover` change the system default input and need `--yes`.
//!
//! ```text
//! cargo run -p prxvc-audio --example routing -- status [--data-dir DIR]
//! cargo run -p prxvc-audio --example routing -- watch [SECONDS]          (read-only)
//! cargo run -p prxvc-audio --example routing -- enter   --yes [--data-dir DIR]
//! cargo run -p prxvc-audio --example routing -- leave   --yes [--data-dir DIR]
//! cargo run -p prxvc-audio --example routing -- recover --yes [--data-dir DIR]
//! ```
//!
//! The data directory defaults to `PRXVC_DATA_DIR`, else the application's
//! directory (`~/Library/Application Support/org.openprx.prx-voice-client`).
//! `enter` switches the default input to the virtual microphone, writing the
//! journal first; the process then waits for Enter before it leaves call mode
//! again, and Ctrl-C leaves the journal for `recover` or the next app start.
//! `leave` restores from the journal of an earlier `enter`; `recover` does what
//! the application does at start.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("the routing example needs macOS");
}

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use prxvc_audio::core_audio_defaults::{DefaultKind, default_entry};
    use prxvc_audio::device_directory::DeviceEntry;
    use prxvc_audio::routing::journal::{FileJournalStore, JournalRead, JournalStore};
    use prxvc_audio::routing::{
        CoreAudioRouting, EnterRequest, OsSystem, RecoveryPlan, RoutingController,
    };
    use prxvc_ports::routing::SystemAudioRouting;

    fn data_dir(args: &[String]) -> PathBuf {
        if let Some(i) = args.iter().position(|a| a == "--data-dir")
            && let Some(dir) = args.get(i + 1)
        {
            return PathBuf::from(dir);
        }
        if let Some(dir) = std::env::var_os("PRXVC_DATA_DIR").filter(|v| !v.is_empty()) {
            return PathBuf::from(dir);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        home.join("Library/Application Support/org.openprx.prx-voice-client")
    }

    fn describe(label: &str, entry: Option<DeviceEntry>) {
        match entry {
            Some(d) => println!(
                "{label}: {} | uid {} | transport {:?}",
                d.name, d.uid, d.transport
            ),
            None => println!("{label}: none"),
        }
    }

    fn controller(
        dir: &std::path::Path,
    ) -> RoutingController<CoreAudioRouting, OsSystem, FileJournalStore> {
        let routing = CoreAudioRouting::new().unwrap_or_else(|e| {
            eprintln!("cannot start routing: {e}");
            std::process::exit(1);
        });
        RoutingController::new(routing, OsSystem, FileJournalStore::in_dir(dir))
    }

    fn require_yes(args: &[String], command: &str) {
        if !args.iter().any(|a| a == "--yes") {
            eprintln!("`{command}` changes the system default input; add --yes to confirm");
            std::process::exit(2);
        }
    }

    fn status(dir: &std::path::Path) {
        describe("default input ", default_entry(DefaultKind::Input));
        describe("default output", default_entry(DefaultKind::Output));
        let store = FileJournalStore::in_dir(dir);
        println!("journal path  : {}", store.path().display());
        match store.read() {
            JournalRead::Absent => println!("journal       : none"),
            JournalRead::Valid(j) => println!(
                "journal       : saved input {:?} ({:?}), our mic {}, pid {}, started_at {}",
                j.saved_default_input_uid,
                j.saved_default_input_name,
                j.our_mic_uid,
                j.pid,
                j.started_at
            ),
            JournalRead::Corrupt { reason } => println!("journal       : unusable ({reason})"),
        }
        let ctl = controller(dir);
        let plan = ctl.recovery_plan();
        println!("recover would : {}", explain(&plan));
    }

    fn explain(plan: &RecoveryPlan) -> String {
        match plan {
            RecoveryPlan::Nothing => "nothing (no journal, default input is not ours)".to_owned(),
            RecoveryPlan::OwnMicrophoneWithoutJournal => {
                "nothing; the default input is the PRX Voice Microphone without a journal (selected by hand?)"
                    .to_owned()
            }
            RecoveryPlan::OwnerAlive { pid } => format!("nothing; the journal belongs to the running process {pid}"),
            RecoveryPlan::ClearStale { crashed_pid } => {
                format!("delete the stale journal of process {crashed_pid}; the default input is not ours, so it stays")
            }
            RecoveryPlan::Restore { saved, crashed_pid } => format!(
                "restore the default input to {} (journal of process {crashed_pid}), else by policy; then delete the journal",
                saved.as_ref().map_or("(none remembered)".to_owned(), |d| format!("{} [{}]", d.name, d.uid))
            ),
            RecoveryPlan::Corrupt { reason, restore } => format!(
                "move the unusable journal aside ({reason}){}",
                if *restore { " and restore the default input by policy" } else { "; the default input stays" }
            ),
            RecoveryPlan::Unreadable(e) => format!("nothing can be decided: {e}"),
        }
    }

    fn watch(seconds: u64) {
        let mut routing = CoreAudioRouting::new().unwrap_or_else(|e| {
            eprintln!("cannot start routing: {e}");
            std::process::exit(1);
        });
        println!(
            "watching for {seconds} s (read-only); change the default device or sleep the machine to see events"
        );
        let end = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < end {
            for change in routing.poll_changes() {
                println!("{change:?}");
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub fn run() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let command = args.first().map_or("status", String::as_str);
        let dir = data_dir(&args);
        match command {
            "status" => status(&dir),
            "watch" => watch(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30)),
            "enter" => {
                require_yes(&args, "enter");
                let mut ctl = controller(&dir);
                match ctl.enter_call_mode(&EnterRequest::default()) {
                    Ok(o) => println!(
                        "call mode on; saved {:?}; physical input {:?}",
                        o.saved, o.physical_input
                    ),
                    Err(e) => return eprintln!("enter failed: {e}"),
                }
                println!(
                    "press Enter to leave call mode (Ctrl-C leaves the journal for `recover`)"
                );
                let mut line = String::new();
                let _ = std::io::stdin().read_line(&mut line);
                match ctl.leave_call_mode() {
                    Ok(o) => println!("left call mode: {o:?}"),
                    Err(e) => eprintln!("leave failed: {e}"),
                }
            }
            "leave" => {
                require_yes(&args, "leave");
                let mut ctl = controller(&dir);
                // A fresh process is not in call mode; leaving is recovery from the journal.
                println!("{:?}", ctl.recover_if_needed());
            }
            "recover" => {
                require_yes(&args, "recover");
                println!("{:?}", controller(&dir).recover_if_needed());
            }
            other => {
                eprintln!("unknown command `{other}`; use status, watch, enter, leave or recover");
                std::process::exit(2);
            }
        }
    }
}

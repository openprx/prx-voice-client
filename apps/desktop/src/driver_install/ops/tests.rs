//! Operation flows of `DriverInstaller` against fake system access and a fake privileged runner.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::driver_install::model::{DRIVER_BUNDLE_NAME, Variant};

const SOURCE: &str =
    "/Applications/PRX Voice.app/Contents/Resources/Drivers/PrxVoiceMicrophone.driver";

fn info(version: &str, variant: Variant) -> DriverInfo {
    DriverInfo {
        identifier: DRIVER_BUNDLE_ID.into(),
        version: version.into(),
        variant,
    }
}

/// The pretend machine shared by both fakes.
struct World {
    bundled: BundleRead,
    installed: BundleRead,
    signature: Result<(), String>,
    executable: bool,
    /// What the audio system reports, one entry per query; the last repeats.
    device: Vec<Result<bool, String>>,
    queries: usize,
    pauses: usize,
    commands: Vec<String>,
    /// Result of the privileged run.
    run: io::Result<RunOutput>,
    /// Whether a successful run changes the machine (installs or removes).
    apply: bool,
}

type Shared = Rc<RefCell<World>>;

fn world() -> Shared {
    Rc::new(RefCell::new(World {
        bundled: BundleRead::Info(info("0.2.0-usb", Variant::Usb)),
        installed: BundleRead::Missing,
        signature: Ok(()),
        executable: true,
        device: vec![Ok(true)],
        queries: 0,
        pauses: 0,
        commands: Vec::new(),
        run: Ok(RunOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        }),
        apply: true,
    }))
}

struct FakeSystem(Shared);
struct FakeRunner(Shared);

impl SystemProbe for FakeSystem {
    fn read_bundle(&self, bundle: &Path) -> BundleRead {
        let w = self.0.borrow();
        if bundle == installed_path() {
            w.installed.clone()
        } else {
            w.bundled.clone()
        }
    }
    fn executable_exists(&self, _bundle: &Path) -> bool {
        self.0.borrow().executable
    }
    fn verify_signature(&self, _bundle: &Path) -> Result<(), String> {
        self.0.borrow().signature.clone()
    }
    fn device_present(&self) -> Result<bool, String> {
        let mut w = self.0.borrow_mut();
        let i = w.queries.min(w.device.len() - 1);
        w.queries += 1;
        w.device[i].clone()
    }
    fn pause(&self, _duration: Duration) {
        self.0.borrow_mut().pauses += 1;
    }
}

impl PrivilegedRunner for FakeRunner {
    fn run(&self, shell_command: &str) -> io::Result<RunOutput> {
        let mut w = self.0.borrow_mut();
        w.commands.push(shell_command.to_owned());
        let result = match &w.run {
            Ok(out) => Ok(out.clone()),
            Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
        };
        if w.apply && matches!(&result, Ok(out) if out.success) {
            w.installed = if shell_command.contains("/usr/bin/ditto") {
                w.bundled.clone()
            } else {
                BundleRead::Missing
            };
        }
        result
    }
}

fn installer(world: &Shared) -> DriverInstaller<FakeSystem, FakeRunner> {
    DriverInstaller::new(
        FakeSystem(world.clone()),
        FakeRunner(world.clone()),
        Some(PathBuf::from(SOURCE)),
    )
    .with_polling(Polling {
        attempts: 3,
        interval: Duration::ZERO,
    })
}

fn failed(stderr: &str) -> io::Result<RunOutput> {
    Ok(RunOutput {
        success: false,
        stdout: String::new(),
        stderr: stderr.to_owned(),
    })
}

#[test]
fn status_reports_action_from_files_only() {
    let w = world();
    let status = installer(&w).status();
    assert_eq!(status.action, DriverAction::Install);
    w.borrow_mut().installed = BundleRead::Info(info("0.1.0-usb", Variant::Usb));
    assert_eq!(installer(&w).status().action, DriverAction::Update);
    w.borrow_mut().installed = BundleRead::Unreadable;
    assert_eq!(installer(&w).status().action, DriverAction::Repair);
    assert!(w.borrow().commands.is_empty());
    assert_eq!(
        w.borrow().queries,
        0,
        "status must not query the audio system"
    );
}

#[test]
fn a_foreign_bundle_at_our_path_is_unreadable() {
    let w = world();
    let mut foreign = info("1.0.0", Variant::Usb);
    foreign.identifier = "com.example.other".into();
    w.borrow_mut().installed = BundleRead::Info(foreign);
    assert_eq!(
        installer(&w).status().installed,
        InstalledDriver::Unreadable
    );
}

#[test]
fn install_runs_the_privileged_command_once_and_verifies_the_device() {
    let w = world();
    let outcome = installer(&w).install().expect("install");
    assert_eq!(outcome.action, DriverAction::Install);
    assert_eq!(outcome.installed, info("0.2.0-usb", Variant::Usb));
    let w = w.borrow();
    assert_eq!(w.commands.len(), 1);
    assert!(w.commands[0].contains(&format!("/usr/bin/ditto '{SOURCE}'")));
}

#[test]
fn update_replaces_an_older_version() {
    let w = world();
    w.borrow_mut().installed = BundleRead::Info(info("0.1.0-usb", Variant::Usb));
    let outcome = installer(&w).install().expect("update");
    assert_eq!(outcome.action, DriverAction::Update);
    assert_eq!(outcome.installed.version, "0.2.0-usb");
}

#[test]
fn variant_change_is_reported_as_such() {
    let w = world();
    w.borrow_mut().installed = BundleRead::Info(info("0.2.0-virtual", Variant::Virtual));
    assert_eq!(
        installer(&w).install().expect("switch").action,
        DriverAction::ChangeVariant
    );
}

#[test]
fn waits_for_the_device_to_appear() {
    let w = world();
    w.borrow_mut().device = vec![Ok(false), Err("profiler busy".into()), Ok(true)];
    installer(&w).install().expect("install");
    assert_eq!(w.borrow().queries, 3);
    assert_eq!(w.borrow().pauses, 2);
}

#[test]
fn device_that_never_appears_is_a_typed_error() {
    let w = world();
    w.borrow_mut().device = vec![Ok(false)];
    assert_eq!(
        installer(&w).install(),
        Err(DriverInstallError::DeviceDidNotAppear)
    );
    assert_eq!(w.borrow().queries, 3);
}

#[test]
fn cancelled_prompt() {
    let w = world();
    w.borrow_mut().run = failed("0:30: execution error: User canceled. (-128)");
    assert_eq!(installer(&w).install(), Err(DriverInstallError::Cancelled));
    assert_eq!(w.borrow().queries, 0);
}

#[test]
fn authorization_refused_and_prompt_that_cannot_start() {
    let w = world();
    w.borrow_mut().run = failed("execution error: The authorization was denied. (-60005)");
    assert!(matches!(
        installer(&w).install(),
        Err(DriverInstallError::AuthorizationFailed(_))
    ));
    w.borrow_mut().run = Err(io::Error::new(io::ErrorKind::NotFound, "osascript missing"));
    assert!(matches!(
        installer(&w).install(),
        Err(DriverInstallError::AuthorizationFailed(d)) if d.contains("osascript missing")
    ));
}

#[test]
fn privileged_step_failures_map_to_typed_errors() {
    let w = world();
    w.borrow_mut().run = failed("execution error: prxvc-failed-step: swap (1)");
    assert!(matches!(
        installer(&w).install(),
        Err(DriverInstallError::CopyFailed { step, .. }) if step == "swap"
    ));
    w.borrow_mut().run = failed("execution error: prxvc-failed-step: restart (1)");
    assert!(matches!(
        installer(&w).install(),
        Err(DriverInstallError::RestartFailed(_))
    ));
    w.borrow_mut().run = failed("execution error: prxvc-failed-step: verify (1)");
    assert!(matches!(
        installer(&w).install(),
        Err(DriverInstallError::SourceRejected(_))
    ));
}

type Mutation = Box<dyn Fn(&mut World)>;

#[test]
fn nothing_privileged_runs_when_the_source_is_unacceptable() {
    let cases: Vec<Mutation> = vec![
        Box::new(|w| w.bundled = BundleRead::Missing),
        Box::new(|w| w.bundled = BundleRead::Unreadable),
        Box::new(|w| {
            let mut foreign = info("0.2.0-usb", Variant::Usb);
            foreign.identifier = "com.example.other".into();
            w.bundled = BundleRead::Info(foreign);
        }),
        Box::new(|w| w.executable = false),
        Box::new(|w| w.signature = Err("invalid signature".into())),
    ];
    for (i, mutate) in cases.iter().enumerate() {
        let w = world();
        mutate(&mut w.borrow_mut());
        let result = installer(&w).install();
        assert!(result.is_err(), "case {i}");
        assert!(
            w.borrow().commands.is_empty(),
            "case {i} ran a privileged command"
        );
    }
}

#[test]
fn missing_payload_is_bundle_not_found() {
    let w = world();
    let unbundled = DriverInstaller::new(FakeSystem(w.clone()), FakeRunner(w.clone()), None);
    assert_eq!(unbundled.install(), Err(DriverInstallError::BundleNotFound));
    w.borrow_mut().bundled = BundleRead::Missing;
    assert_eq!(
        installer(&w).install(),
        Err(DriverInstallError::BundleNotFound)
    );
}

#[test]
fn hostile_bundle_paths_never_reach_the_runner_unquoted() {
    let w = world();
    for bad in [
        "relative/PrxVoiceMicrophone.driver",
        "/tmp/a\nb/PrxVoiceMicrophone.driver",
        "/tmp/Other.driver",
    ] {
        let i = DriverInstaller::new(
            FakeSystem(w.clone()),
            FakeRunner(w.clone()),
            Some(PathBuf::from(bad)),
        );
        assert!(
            matches!(i.install(), Err(DriverInstallError::SourceRejected(_))),
            "{bad:?}"
        );
    }
    assert!(w.borrow().commands.is_empty());
    let quote_path = "/tmp/it's `x` $y/PrxVoiceMicrophone.driver";
    let i = DriverInstaller::new(
        FakeSystem(w.clone()),
        FakeRunner(w.clone()),
        Some(PathBuf::from(quote_path)),
    );
    i.install().expect("quoted path installs");
    assert!(
        w.borrow().commands[0]
            .contains("/usr/bin/ditto '/tmp/it'\\''s `x` $y/PrxVoiceMicrophone.driver' ")
    );
}

#[test]
fn the_team_requirement_reaches_the_command() {
    let w = world();
    installer(&w)
        .with_team_id("ABCDE12345")
        .install()
        .expect("install");
    assert!(w.borrow().commands[0].contains("-R '=anchor apple generic"));
}

#[test]
fn installed_bundle_that_differs_after_the_copy_is_reported() {
    let w = world();
    w.borrow_mut().apply = false;
    w.borrow_mut().installed = BundleRead::Info(info("0.1.0-usb", Variant::Usb));
    assert!(matches!(
        installer(&w).install(),
        Err(DriverInstallError::CopyFailed { step, .. }) if step == "check"
    ));
}

#[test]
fn uninstall_removes_and_verifies() {
    let w = world();
    w.borrow_mut().installed = BundleRead::Info(info("0.2.0-usb", Variant::Usb));
    w.borrow_mut().device = vec![Ok(true), Ok(false)];
    assert_eq!(installer(&w).uninstall(), Ok(UninstallOutcome::Removed));
    assert_eq!(w.borrow().commands.len(), 1);
    assert!(
        w.borrow().commands[0]
            .contains("/bin/rm -rf '/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver'")
    );
}

#[test]
fn uninstall_without_a_driver_shows_no_prompt() {
    let w = world();
    assert_eq!(
        installer(&w).uninstall(),
        Ok(UninstallOutcome::NotInstalled)
    );
    assert!(w.borrow().commands.is_empty());
}

#[test]
fn uninstall_failures() {
    let w = world();
    w.borrow_mut().installed = BundleRead::Info(info("0.2.0-usb", Variant::Usb));
    w.borrow_mut().run = failed("execution error: User canceled. (-128)");
    assert_eq!(
        installer(&w).uninstall(),
        Err(DriverInstallError::Cancelled)
    );

    w.borrow_mut().run = failed("execution error: prxvc-failed-step: remove (1)");
    assert!(matches!(
        installer(&w).uninstall(),
        Err(DriverInstallError::CopyFailed { .. })
    ));

    let w = world();
    w.borrow_mut().installed = BundleRead::Info(info("0.2.0-usb", Variant::Usb));
    w.borrow_mut().device = vec![Ok(true)];
    assert_eq!(
        installer(&w).uninstall(),
        Err(DriverInstallError::DeviceStillPresent)
    );

    // Unreadable leftovers are removed too.
    let w = world();
    w.borrow_mut().installed = BundleRead::Unreadable;
    w.borrow_mut().device = vec![Ok(false)];
    assert_eq!(installer(&w).uninstall(), Ok(UninstallOutcome::Removed));
}

#[test]
fn bundle_name_constant_matches_the_source_path() {
    assert!(SOURCE.ends_with(DRIVER_BUNDLE_NAME));
}

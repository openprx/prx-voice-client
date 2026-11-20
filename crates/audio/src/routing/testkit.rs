//! Test doubles for the routing controller: a probe with a scripted machine, an in-memory journal that logs what the system looked like at each write, and a port wrapper that can ignore switches.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use prxvc_ports::fakes::routing::{FakeSystemAudioRouting, RoutingProbe};
use prxvc_ports::routing::{
    BundleId, CallActivity, DeviceUid, RoutingChange, RoutingError, SystemAudioRouting,
};

use super::controller::RoutingController;
use super::journal::{Journal, JournalError, JournalRead, JournalStore, SCHEMA_VERSION};
use super::system::SystemProbe;
use crate::device_directory::DeviceEntry;
use crate::driver_status::{BundleState, DriverStatus, parse_bundle_info};
use crate::input_policy::{InputTransport, OWN_MIC_NAME, OWN_MIC_SINK_UID, OWN_MIC_UID};

/// The process ID the controller under test runs as.
pub const OWN_PID: u32 = 100;

pub fn uid(s: &str) -> DeviceUid {
    DeviceUid::new(s)
}

fn entry(
    uid: &str,
    name: &str,
    transport: InputTransport,
    input: bool,
    hidden: bool,
) -> DeviceEntry {
    DeviceEntry {
        uid: uid.to_owned(),
        name: name.to_owned(),
        transport,
        has_input: input,
        has_output: !input,
        is_alive: true,
        is_hidden: hidden,
    }
}

/// The machine of the tests: a built-in microphone, a USB microphone and our driver's two devices.
pub fn default_devices() -> Vec<DeviceEntry> {
    vec![
        entry(
            "builtin",
            "Built-in Mic",
            InputTransport::BuiltIn,
            true,
            false,
        ),
        entry("usb", "USB Mic", InputTransport::Usb, true, false),
        entry(OWN_MIC_UID, OWN_MIC_NAME, InputTransport::Usb, true, false),
        entry(
            OWN_MIC_SINK_UID,
            "PRX Voice Sink",
            InputTransport::Usb,
            false,
            true,
        ),
    ]
}

fn installed_bundle() -> BundleState {
    let plist = "<plist version=\"1.0\"><dict><key>CFBundleShortVersionString</key><string>0.1.0-usb</string>\
                 <key>CFBundleVersion</key><string>1</string><key>PrxTransportVariant</key><string>usb</string></dict></plist>";
    BundleState::Installed(parse_bundle_info(plist.as_bytes()).expect("test plist"))
}

struct SystemInner {
    devices: Vec<DeviceEntry>,
    bundle: BundleState,
    alive: HashSet<u32>,
    now: u64,
    fail_devices: bool,
}

/// [`SystemProbe`] with scripted devices, driver bundle, live processes and clock.
#[derive(Clone)]
pub struct FakeSystem {
    inner: Arc<Mutex<SystemInner>>,
}

impl FakeSystem {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SystemInner {
                devices: default_devices(),
                bundle: installed_bundle(),
                alive: HashSet::from([OWN_PID]),
                now: 1_790_000_000,
                fail_devices: false,
            })),
        }
    }

    pub fn set_bundle(&self, bundle: BundleState) {
        self.inner.lock().bundle = bundle;
    }

    pub fn set_alive(&self, pid: u32, alive: bool) {
        let mut s = self.inner.lock();
        if alive {
            s.alive.insert(pid);
        } else {
            s.alive.remove(&pid);
        }
    }

    pub fn remove_device(&self, uid: &str) {
        self.inner.lock().devices.retain(|d| d.uid != uid);
    }

    pub fn keep_only(&self, uids: &[&str]) {
        self.inner
            .lock()
            .devices
            .retain(|d| uids.contains(&d.uid.as_str()));
    }

    pub fn set_dead(&self, uid: &str) {
        for d in self
            .inner
            .lock()
            .devices
            .iter_mut()
            .filter(|d| d.uid == uid)
        {
            d.is_alive = false;
        }
    }

    pub fn add_device(&self, uid: &str) {
        let device = default_devices()
            .into_iter()
            .find(|d| d.uid == uid)
            .expect("a known test device");
        self.inner.lock().devices.push(device);
    }
}

impl SystemProbe for FakeSystem {
    fn devices(&self) -> Result<Vec<DeviceEntry>, RoutingError> {
        let s = self.inner.lock();
        if s.fail_devices {
            return Err(RoutingError::Unavailable {
                reason: "no list".to_owned(),
            });
        }
        Ok(s.devices.clone())
    }

    fn driver_status(&self) -> DriverStatus {
        let s = self.inner.lock();
        DriverStatus::assess(s.bundle.clone(), &s.devices, "0.1.0")
    }

    fn own_pid(&self) -> u32 {
        OWN_PID
    }

    fn process_alive(&self, pid: u32) -> bool {
        self.inner.lock().alive.contains(&pid)
    }

    fn unix_secs(&self) -> u64 {
        self.inner.lock().now
    }
}

/// One journal operation and the system as it was at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalLogEntry {
    pub op: &'static str,
    /// The default input when the operation happened.
    pub default_input: Option<DeviceUid>,
    /// How many switches the system had accepted by then.
    pub switches_so_far: usize,
}

struct JournalInner {
    content: JournalRead,
    log: Vec<JournalLogEntry>,
    fail_write: bool,
    fail_remove: bool,
}

/// In-memory [`JournalStore`] that logs each operation against the routing fake.
#[derive(Clone)]
pub struct MemJournal {
    inner: Arc<Mutex<JournalInner>>,
    probe: RoutingProbe,
}

impl MemJournal {
    fn new(probe: RoutingProbe) -> Self {
        Self {
            inner: Arc::new(Mutex::new(JournalInner {
                content: JournalRead::Absent,
                log: Vec::new(),
                fail_write: false,
                fail_remove: false,
            })),
            probe,
        }
    }

    fn log(&self, op: &'static str) {
        let entry = JournalLogEntry {
            op,
            default_input: self.probe.default_input(),
            switches_so_far: self.probe.set_calls().len(),
        };
        self.inner.lock().log.push(entry);
    }

    pub fn content(&self) -> JournalRead {
        self.inner.lock().content.clone()
    }

    pub fn set_content(&self, content: JournalRead) {
        self.inner.lock().content = content;
    }

    pub fn log_entries(&self) -> Vec<JournalLogEntry> {
        self.inner.lock().log.clone()
    }

    pub fn fail_writes(&self, fail: bool) {
        self.inner.lock().fail_write = fail;
    }

    pub fn fail_removes(&self, fail: bool) {
        self.inner.lock().fail_remove = fail;
    }
}

impl JournalStore for MemJournal {
    fn read(&self) -> JournalRead {
        self.inner.lock().content.clone()
    }

    fn write(&self, journal: &Journal) -> Result<(), JournalError> {
        self.log("write");
        let mut s = self.inner.lock();
        if s.fail_write {
            return Err(JournalError("disk full".to_owned()));
        }
        s.content = JournalRead::Valid(journal.clone());
        Ok(())
    }

    fn remove(&self) -> Result<(), JournalError> {
        self.log("remove");
        let mut s = self.inner.lock();
        if s.fail_remove {
            return Err(JournalError("read-only".to_owned()));
        }
        s.content = JournalRead::Absent;
        Ok(())
    }

    fn quarantine(&self, stamp: u64) -> Result<PathBuf, JournalError> {
        self.log("quarantine");
        self.inner.lock().content = JournalRead::Absent;
        Ok(PathBuf::from(format!(
            "routing-journal.corrupt-{stamp}.json"
        )))
    }
}

/// The routing fake with a switch that can be made to have no effect.
pub struct SpyRouting {
    inner: FakeSystemAudioRouting,
    ignore_sets: Arc<AtomicBool>,
}

impl SystemAudioRouting for SpyRouting {
    fn default_input(&self) -> Result<DeviceUid, RoutingError> {
        self.inner.default_input()
    }

    fn set_default_input(&self, uid: &DeviceUid) -> Result<(), RoutingError> {
        if self.ignore_sets.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.inner.set_default_input(uid)
    }

    fn device_exists(&self, uid: &DeviceUid) -> bool {
        self.inner.device_exists(uid)
    }

    fn call_activity(&self, app: &BundleId) -> Result<CallActivity, RoutingError> {
        self.inner.call_activity(app)
    }

    fn poll_changes(&mut self) -> Vec<RoutingChange> {
        self.inner.poll_changes()
    }
}

/// The handles of one test.
pub struct Rig {
    pub probe: RoutingProbe,
    pub system: FakeSystem,
    pub journal: MemJournal,
    ignore_sets: Arc<AtomicBool>,
}

impl Rig {
    /// The default input as the "system" has it.
    pub fn default_input(&self) -> Option<DeviceUid> {
        self.probe.default_input()
    }

    /// Switches the system accepted.
    pub fn switches(&self) -> Vec<DeviceUid> {
        self.probe.set_calls()
    }

    /// A device disappears from both the routing fake and the device list.
    pub fn unplug(&self, uid: &str) {
        self.probe.unplug(uid);
        self.system.remove_device(uid);
    }

    /// A known test device appears in both the routing fake and the device list.
    pub fn plug(&self, uid: &str) {
        self.probe.plug(uid);
        self.system.add_device(uid);
    }

    /// Accepted switches stop having any effect (the system reverts them).
    pub fn ignore_switches(&self, ignore: bool) {
        self.ignore_sets.store(ignore, Ordering::Relaxed);
    }
}

pub type TestController = RoutingController<SpyRouting, FakeSystem, MemJournal>;

/// A controller on a machine whose default input is `default_input`.
pub fn controller_with_default(default_input: &str) -> (TestController, Rig) {
    let routing = FakeSystemAudioRouting::new(
        default_input,
        &["builtin", "usb", OWN_MIC_UID, OWN_MIC_SINK_UID],
    );
    let probe = routing.probe();
    let ignore_sets = Arc::new(AtomicBool::new(false));
    let system = FakeSystem::new();
    let journal = MemJournal::new(probe.clone());
    let controller = RoutingController::new(
        SpyRouting {
            inner: routing,
            ignore_sets: Arc::clone(&ignore_sets),
        },
        system.clone(),
        journal.clone(),
    );
    (
        controller,
        Rig {
            probe,
            system,
            journal,
            ignore_sets,
        },
    )
}

/// A controller on a machine whose default input is the built-in microphone.
pub fn controller() -> (TestController, Rig) {
    controller_with_default("builtin")
}

/// A well-formed journal.
pub fn journal(pid: u32, saved: Option<&str>) -> Journal {
    Journal {
        schema_version: SCHEMA_VERSION,
        saved_default_input_uid: saved.map(str::to_owned),
        saved_default_input_name: saved.map(|s| format!("name of {s}")),
        our_mic_uid: OWN_MIC_UID.to_owned(),
        pid,
        started_at: 1_700_000_000,
    }
}

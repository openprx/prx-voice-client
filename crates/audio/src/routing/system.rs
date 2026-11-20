//! What the controller asks of the machine besides the default input: the device list, the driver status, process liveness and the wall clock.
//!
//! [`SystemProbe`] is the small internal seam that keeps the controller free of
//! Core Audio and of the real clock; [`OsSystem`] is the real answer, the fake
//! lives in the tests.

use prxvc_ports::routing::RoutingError;

use crate::device_directory::{DeviceEntry, list_devices_with};
use crate::driver_status::DriverStatus;
use crate::input_policy::{OWN_MIC_SINK_UID, OWN_MIC_UID};

/// The machine facts the controller reads.
pub trait SystemProbe: Send {
    /// Every audio device, hidden ones of ours included.
    ///
    /// # Errors
    /// [`RoutingError::Unavailable`] when the list cannot be read.
    fn devices(&self) -> Result<Vec<DeviceEntry>, RoutingError>;

    /// The virtual microphone driver's state now.
    fn driver_status(&self) -> DriverStatus;

    /// The process ID of this process.
    fn own_pid(&self) -> u32;

    /// Whether a process with this ID is running.
    fn process_alive(&self, pid: u32) -> bool;

    /// Seconds since the Unix epoch.
    fn unix_secs(&self) -> u64;
}

/// [`SystemProbe`] on the real machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsSystem;

impl SystemProbe for OsSystem {
    fn devices(&self) -> Result<Vec<DeviceEntry>, RoutingError> {
        list_devices_with(&[OWN_MIC_UID, OWN_MIC_SINK_UID]).map_err(|e| RoutingError::Unavailable {
            reason: e.to_string(),
        })
    }

    fn driver_status(&self) -> DriverStatus {
        DriverStatus::detect()
    }

    fn own_pid(&self) -> u32 {
        std::process::id()
    }

    fn process_alive(&self, pid: u32) -> bool {
        process_alive(pid)
    }

    fn unix_secs(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 only checks that the process exists and may be signalled; it sends nothing.
    let rc = unsafe { libc::kill(pid, 0) };
    // EPERM: the process exists but belongs to someone else, which is still alive.
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    // Without a liveness check the safe answer is "alive": the journal is then left alone.
    true
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive_and_an_impossible_pid_is_not() {
        let sys = OsSystem;
        assert!(sys.process_alive(sys.own_pid()));
        assert!(!sys.process_alive(0));
        assert!(!sys.process_alive(u32::MAX));
    }
}

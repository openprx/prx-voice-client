//! Core Audio reads of an application's audio activity (macOS): safe wrapper over `native/prxvc_procs.m`.
//!
//! Read-only: the shim creates no tap, device or stream. The row callback runs
//! on the calling thread while the shim enumerates, and only copies the row
//! into a `Vec`; nothing here runs on a real-time thread.

use std::ffi::{CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::model::{AppAudioActivity, ProcessDevice};
use super::watcher::{ActivityError, ActivitySource};
use crate::tap_types::TapTarget;

const MAX_DEVICES: usize = 16;
const STR_LEN: usize = 128;
const BUNDLE_LEN: usize = 256;

#[repr(C)]
#[derive(Clone, Copy)]
struct RawDevice {
    object_id: u32,
    uid: [c_char; STR_LEN],
    name: [c_char; STR_LEN],
    input_channels: u32,
    output_channels: u32,
}

#[repr(C)]
struct RawRow {
    object_id: u32,
    pid: i32,
    bundle_id: [c_char; BUNDLE_LEN],
    is_running: i32,
    is_running_input: i32,
    is_running_output: i32,
    input_count: u32,
    output_count: u32,
    inputs: [RawDevice; MAX_DEVICES],
    outputs: [RawDevice; MAX_DEVICES],
}

type RowCb = extern "C" fn(ctx: *mut c_void, row: *const RawRow);

unsafe extern "C" {
    fn prxvc_procs_enumerate(
        bundle_id: *const c_char,
        pid: i32,
        with_devices: i32,
        cb: RowCb,
        ctx: *mut c_void,
    ) -> i32;
}

/// A NUL-terminated C char array as text (the part before the first NUL).
fn text(raw: &[c_char]) -> String {
    let bytes: Vec<u8> = raw.iter().map(|&c| c as u8).take_while(|&b| b != 0).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn devices(raw: &[RawDevice], count: u32) -> Vec<ProcessDevice> {
    raw.iter()
        .take((count as usize).min(MAX_DEVICES))
        .map(|d| ProcessDevice {
            object_id: d.object_id,
            uid: text(&d.uid),
            name: text(&d.name),
            input_channels: d.input_channels,
            output_channels: d.output_channels,
        })
        .collect()
}

fn activity_of(row: &RawRow) -> AppAudioActivity {
    AppAudioActivity {
        bundle_id: text(&row.bundle_id),
        present: true,
        pid: Some(row.pid),
        audio_running: row.is_running != 0,
        input_running: row.is_running_input != 0,
        output_running: row.is_running_output != 0,
        input_devices: devices(&row.inputs, row.input_count),
        output_devices: devices(&row.outputs, row.output_count),
    }
}

/// Collects the rows the shim reports. Runs on the calling thread.
extern "C" fn collect_row(ctx: *mut c_void, row: *const RawRow) {
    if ctx.is_null() || row.is_null() {
        return;
    }
    // A panic must never unwind into C.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `ctx` is the `&mut Vec<AppAudioActivity>` passed by `enumerate`,
        // alive for the whole enumeration and used only from this thread.
        let rows = unsafe { &mut *(ctx as *mut Vec<AppAudioActivity>) };
        // SAFETY: the shim passes a valid row for the duration of the call.
        rows.push(activity_of(unsafe { &*row }));
    }));
}

/// Rows of the process objects matching the filter.
fn enumerate(bundle_id: Option<&str>, pid: i32, with_devices: bool) -> Result<Vec<AppAudioActivity>, ActivityError> {
    let bundle = match bundle_id {
        Some(id) => Some(CString::new(id).map_err(|_| ActivityError::ReadFailed { status: -1 })?),
        None => None,
    };
    let mut rows: Vec<AppAudioActivity> = Vec::new();
    // SAFETY: the bundle pointer is NUL-terminated or null; `rows` outlives the
    // call and is only touched by `collect_row` on this thread during it.
    let status = unsafe {
        prxvc_procs_enumerate(
            bundle.as_ref().map_or(std::ptr::null(), |b| b.as_ptr()),
            pid,
            i32::from(with_devices),
            collect_row,
            (&mut rows as *mut Vec<AppAudioActivity>).cast::<c_void>(),
        )
    };
    if status != 0 {
        return Err(ActivityError::ReadFailed { status });
    }
    Ok(rows)
}

/// Read the activity of the application with `bundle_id`: merged over its
/// process objects, with the devices in use. An application the audio system
/// does not know is [`AppAudioActivity::absent`].
///
/// # Errors
/// [`ActivityError::ReadFailed`] when the process list cannot be read (for
/// example before macOS 14.4).
pub fn read_app_activity(bundle_id: &str) -> Result<AppAudioActivity, ActivityError> {
    let mut merged = AppAudioActivity::absent(bundle_id);
    for row in enumerate(Some(bundle_id), 0, true)? {
        merged.merge(row);
    }
    Ok(merged)
}

/// Whether the tap target's output is running now, without reading devices.
/// `false` when the target is unknown to the audio system or the read fails
/// (the zero watchdog then does not act).
pub fn target_output_running(target: &TapTarget) -> bool {
    let rows = match target {
        TapTarget::BundleId(id) => enumerate(Some(id), 0, false),
        // A pid filter of 0 or less would match every process.
        TapTarget::Pid(pid) if *pid <= 0 => return false,
        TapTarget::Pid(pid) => enumerate(None, *pid, false),
    };
    rows.is_ok_and(|rows| rows.iter().any(|r| r.output_running))
}

/// [`ActivitySource`] reading one application through Core Audio.
pub struct NativeActivitySource {
    bundle_id: String,
}

impl NativeActivitySource {
    /// A source for the application with `bundle_id`.
    pub fn new(bundle_id: impl Into<String>) -> Self {
        Self { bundle_id: bundle_id.into() }
    }
}

impl ActivitySource for NativeActivitySource {
    fn read(&mut self) -> Result<AppAudioActivity, ActivityError> {
        read_app_activity(&self.bundle_id)
    }
}

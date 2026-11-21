//! Core Audio process-tap capture (macOS 14.4+): safe wrapper over `native/prxvc_tap.m` that delivers one application's audio as frames.
//!
//! The IO callback ([`crate::tap_rt::on_buffers`]) runs on the Core Audio HAL
//! thread, a real-time thread: it only validates the buffer shape, copies
//! samples into a preallocated lock-free ring ([`crate::tap_ring`]) and bumps
//! atomic counters. It lives in a `*_rt.rs` file and is covered by the
//! real-time check (`tests/realtime_files.rs`). This file holds the FFI
//! declarations, the callback context and everything that runs on normal
//! threads (frame building, health checks, re-creation; see
//! [`crate::tap_supervisor`]).
//!
//! Frames are emitted as raw interleaved **float32 at the tap's native rate**
//! (typically 48 kHz stereo); downmix-to-mono and resampling to 16 kHz for ASR
//! happen downstream.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::tap_consent::start_gate;
pub use crate::tap_consent::{ConsentOutcome, PermissionStatus, permission_status, request_capture_consent};
use crate::tap_format::{RawFormat, StreamLayout};
use crate::tap_ring::{RingConsumer, RingProducer, SLOT_SAMPLES, tap_ring};
use crate::tap_rt::on_buffers;
use crate::tap_incidents::{TapIncidentCounts, TapIncidents};
use crate::tap_supervisor::{Supervisor, spawn_supervisor};
use crate::tap_types::{TapConfig, TapCounters, TapError, TapEvent, TapStats, TapTarget};
use crate::{AudioFormat, AudioFrame};

/// Slots in the ring between the IO thread and the consumer: about 1.5 s of
/// 512-frame callbacks at 48 kHz.
const RING_SLOTS: usize = 128;

// ---------------------------------------------------------------------------
// Raw FFI surface (see native/prxvc_tap.h)
// ---------------------------------------------------------------------------

/// Opaque handle owned by the Objective-C shim.
#[repr(C)]
pub(crate) struct PrxvcTap {
    _private: [u8; 0],
}

#[repr(C)]
struct RawTapConfig {
    pid: c_int,
    bundle_id: *const c_char,
    prefer_bundle_form: i32,
    mute: i32,
}

#[repr(C)]
#[derive(Default)]
struct RawTapFormat {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    channels: u32,
    bits_per_channel: u32,
    bytes_per_frame: u32,
}

#[repr(C)]
struct RawTapResult {
    status: i32,
    os_status: i32,
    step: [c_char; 48],
    used_bundle_form: i32,
}

#[repr(C)]
pub(crate) struct RawTapBuffer {
    pub(crate) channels: u32,
    pub(crate) byte_size: u32,
    pub(crate) data: *const c_void,
}

/// `host_time_ns` is the capture time of the cycle's first frame on the Core
/// Audio host clock in nanoseconds, 0 if the system gave none.
type TapAudioCb = extern "C" fn(ctx: *mut c_void, count: u32, buffers: *const RawTapBuffer, host_time_ns: u64);

const ERR_UNSUPPORTED_OS: i32 = 1;
const ERR_PROCESS_NOT_FOUND: i32 = 2;
const ERR_CORE_AUDIO: i32 = 3;

unsafe extern "C" {
    fn prxvc_tap_create(
        config: *const RawTapConfig,
        cb: TapAudioCb,
        ctx: *mut c_void,
        format: *mut RawTapFormat,
        result: *mut RawTapResult,
    ) -> *mut PrxvcTap;
    fn prxvc_tap_start(tap: *mut PrxvcTap) -> i32;
    fn prxvc_tap_stop(tap: *mut PrxvcTap);
    fn prxvc_tap_destroy(tap: *mut PrxvcTap);
    fn prxvc_tap_target_running(bundle_id: *const c_char, pid: c_int, pid_out: *mut c_int) -> i32;
    fn prxvc_tap_bundle_form_available() -> i32;
    fn prxvc_tap_host_time_now_ns() -> u64;
}

// ---------------------------------------------------------------------------
// Callback context and the real-time callback
// ---------------------------------------------------------------------------

/// State handed to the C callback as its opaque `ctx`. Lives in an [`Arc`] for
/// the whole supervision, so it outlives every native tap that uses it.
pub(crate) struct TapCtx {
    /// Packed [`StreamLayout`] of the current native tap; `0` while none is installed.
    pub(crate) layout: AtomicU64,
    /// Written only by the IO callback, which the HAL calls serially, and only
    /// while a native tap is running; see [`crate::tap_rt::on_buffers`].
    pub(crate) producer: std::cell::UnsafeCell<RingProducer>,
    pub(crate) counters: Arc<TapCounters>,
}

// SAFETY: `producer` is touched only by the IO callback (one thread at a time,
// and never after `prxvc_tap_stop` returned); every other field is atomic.
unsafe impl Sync for TapCtx {}
// SAFETY: see above; the context owns plain data.
unsafe impl Send for TapCtx {}

impl TapCtx {
    fn new(producer: RingProducer) -> Self {
        Self {
            layout: AtomicU64::new(0),
            producer: std::cell::UnsafeCell::new(producer),
            counters: Arc::new(TapCounters::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// Native tap handle
// ---------------------------------------------------------------------------

/// One native tap with its aggregate device and IO proc. Stops and destroys
/// them on drop.
pub(crate) struct NativeTap {
    handle: *mut PrxvcTap,
    /// Whether the bundle-ID form with process restore is in use.
    pub(crate) bundle_form: bool,
}

// SAFETY: the handle is used only from the supervisor/creating thread (create,
// start, stop, destroy); the HAL thread only sees the context, never this handle.
unsafe impl Send for NativeTap {}

impl NativeTap {
    /// Create and start a native tap for `config`, delivering into `ctx`.
    /// `check_consent` runs the consent gate first (see [`crate::tap_consent`]).
    pub(crate) fn open(config: &TapConfig, ctx: &Arc<TapCtx>, check_consent: bool) -> Result<Self, TapError> {
        if check_consent {
            start_gate(permission_status(), crate::tap_consent::bundle_declares_capture_usage())?;
        }
        let (bundle, pid) = match &config.target {
            TapTarget::Pid(pid) => (None, *pid),
            TapTarget::BundleId(id) => (
                Some(CString::new(id.as_str()).map_err(|_| TapError::Internal("bundle id contains NUL".into()))?),
                0,
            ),
        };
        let raw_config = RawTapConfig {
            pid,
            bundle_id: bundle.as_ref().map_or(std::ptr::null(), |b| b.as_ptr()),
            prefer_bundle_form: i32::from(bundle.is_some()),
            mute: config.mute.to_native(),
        };
        let mut format = RawTapFormat::default();
        let mut result = RawTapResult { status: 0, os_status: 0, step: [0; 48], used_bundle_form: 0 };
        // SAFETY: all pointers are valid for the call; `ctx` outlives the tap
        // (the supervisor keeps its `Arc` until after this `NativeTap` is dropped).
        let handle = unsafe {
            prxvc_tap_create(
                &raw_config,
                on_buffers,
                Arc::as_ptr(ctx) as *mut c_void,
                &mut format,
                &mut result,
            )
        };
        if handle.is_null() {
            return Err(map_failure(&result, &config.target));
        }
        let tap = Self { handle, bundle_form: result.used_bundle_form != 0 };
        let layout = StreamLayout::validate(&RawFormat {
            sample_rate: format.sample_rate,
            format_id: format.format_id,
            format_flags: format.format_flags,
            channels: format.channels,
            bits_per_channel: format.bits_per_channel,
            bytes_per_frame: format.bytes_per_frame,
        })?;
        ctx.layout.store(layout.pack(), Ordering::Release);
        // SAFETY: `handle` is a valid tap created above.
        let status = unsafe { prxvc_tap_start(tap.handle) };
        if status != 0 {
            ctx.layout.store(0, Ordering::Release);
            return Err(map_os_failure("start IO", status));
        }
        Ok(tap)
    }
}

impl Drop for NativeTap {
    fn drop(&mut self) {
        // SAFETY: `handle` is valid until destroyed here; stop returns only after
        // the IO proc stopped, so no callback runs during or after destroy.
        unsafe {
            prxvc_tap_stop(self.handle);
            prxvc_tap_destroy(self.handle);
        }
    }
}

/// Turn a failed create into a typed error; a denied permission wins.
fn map_failure(result: &RawTapResult, target: &TapTarget) -> TapError {
    match result.status {
        ERR_UNSUPPORTED_OS => TapError::UnsupportedOs,
        ERR_PROCESS_NOT_FOUND => TapError::ProcessNotFound { target: target.describe() },
        ERR_CORE_AUDIO => {
            if permission_status() == PermissionStatus::Denied {
                return TapError::PermissionDenied;
            }
            // SAFETY: the shim NUL-terminates `step`, which is zero-initialised.
            let step = unsafe { CStr::from_ptr(result.step.as_ptr()) }.to_string_lossy().into_owned();
            TapError::CoreAudio { operation: step, status: result.os_status }
        }
        _ => TapError::Internal("native tap creation failed".into()),
    }
}

fn map_os_failure(operation: &str, status: i32) -> TapError {
    if permission_status() == PermissionStatus::Denied {
        return TapError::PermissionDenied;
    }
    TapError::CoreAudio { operation: operation.into(), status }
}

/// The Core Audio host clock now, in nanoseconds: the clock of the capture
/// times returned by [`TapReceiver::try_recv_stamped`].
pub fn host_time_now_ns() -> u64 {
    // SAFETY: no arguments; reads the host clock.
    unsafe { prxvc_tap_host_time_now_ns() }
}

/// Whether the OS provides the bundle-ID tap form with process restore (macOS 26+).
pub fn bundle_form_available() -> bool {
    // SAFETY: no arguments; a pure version check.
    unsafe { prxvc_tap_bundle_form_available() != 0 }
}

/// Whether the target has a Core Audio process object (is running).
pub(crate) fn target_running(target: &TapTarget) -> bool {
    match target {
        TapTarget::Pid(pid) => {
            // SAFETY: null bundle id and null pid_out are accepted.
            unsafe { prxvc_tap_target_running(std::ptr::null(), *pid, std::ptr::null_mut()) != 0 }
        }
        TapTarget::BundleId(id) => CString::new(id.as_str()).is_ok_and(|c| {
            // SAFETY: `c` is a valid NUL-terminated string; null pid_out is accepted.
            unsafe { prxvc_tap_target_running(c.as_ptr(), 0, std::ptr::null_mut()) != 0 }
        }),
    }
}

// ---------------------------------------------------------------------------
// Public handle
// ---------------------------------------------------------------------------

/// A supervised Core Audio process tap. Stops the supervisor and releases all
/// native resources on drop. Construct with [`CoreAudioTap::start`].
pub struct CoreAudioTap {
    counters: Arc<TapCounters>,
    incidents: Arc<TapIncidents>,
    events: Receiver<TapEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CoreAudioTap {
    /// Start capturing the configured application.
    ///
    /// Returns the tap plus a [`TapReceiver`] of captured frames (interleaved
    /// float32 at the tap's native rate). The tap is supervised: a stalled or
    /// vanished target is reported through [`events`](Self::events) and the tap
    /// is re-created when the target runs again. Keep the [`CoreAudioTap`]
    /// alive for as long as you want frames; dropping it stops capture.
    ///
    /// Consent is checked first and never requested here: use
    /// [`request_capture_consent`] separately.
    ///
    /// # Errors
    /// A [`TapError`]: unsupported OS, permission denied (with the text a user
    /// needs), consent not yet requested, not running from an application bundle
    /// that can ask, target not running, unsupported stream format, or a Core
    /// Audio failure.
    pub fn start(config: TapConfig) -> Result<(Self, TapReceiver), TapError> {
        Self::start_checked(config, true, Arc::default())
    }

    /// Like [`start`](Self::start), with the counters of re-creations shared with the caller.
    ///
    /// # Errors
    /// As [`start`](Self::start).
    pub fn start_counted(
        config: TapConfig,
        incidents: Arc<TapIncidents>,
    ) -> Result<(Self, TapReceiver), TapError> {
        Self::start_checked(config, true, incidents)
    }

    /// Like [`start`](Self::start) but without the consent gate: the tap is
    /// created even when consent is known to be missing, so that the digital-zero
    /// watchdog can be exercised (`live_tap_zero_detect`). Not for production.
    ///
    /// # Errors
    /// As [`start`](Self::start), except for the consent errors.
    pub fn start_without_consent_check(config: TapConfig) -> Result<(Self, TapReceiver), TapError> {
        Self::start_checked(config, false, Arc::default())
    }

    fn start_checked(
        config: TapConfig,
        check_consent: bool,
        incidents: Arc<TapIncidents>,
    ) -> Result<(Self, TapReceiver), TapError> {
        let (producer, consumer) = tap_ring(RING_SLOTS);
        let ctx = Arc::new(TapCtx::new(producer));
        let native = NativeTap::open(&config, &ctx, check_consent)?;
        let counters = Arc::clone(&ctx.counters);
        let (events_tx, events) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = spawn_supervisor(Supervisor {
            config,
            ctx,
            check_consent,
            native: Some(native),
            events: events_tx,
            stop: Arc::clone(&stop),
            incidents: Arc::clone(&incidents),
        })?;
        Ok((Self { counters, incidents, events, stop, thread: Some(thread) }, TapReceiver::new(consumer)))
    }

    /// Health events (stall, target gone, restart) since the last call.
    pub fn events(&self) -> &Receiver<TapEvent> {
        &self.events
    }

    /// Callback, frame and overrun counters.
    pub fn stats(&self) -> TapStats {
        self.counters.snapshot()
    }

    /// Re-creations of the tap by reason and the longest capture interruption they caused.
    pub fn incidents(&self) -> TapIncidentCounts {
        self.incidents.snapshot()
    }
}

impl Drop for CoreAudioTap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join(); // a panicked supervisor has nothing left to release
        }
    }
}

/// Consumer end of the tap: turns ring slots into [`AudioFrame`]s.
pub struct TapReceiver {
    consumer: RingConsumer,
    scratch: Vec<f32>,
    sequence: u64,
}

impl TapReceiver {
    fn new(consumer: RingConsumer) -> Self {
        Self { consumer, scratch: Vec::with_capacity(SLOT_SAMPLES), sequence: 0 }
    }

    /// The next captured frame, if one is ready.
    pub fn try_recv(&mut self) -> Option<AudioFrame> {
        self.try_recv_stamped().map(|(frame, _)| frame)
    }

    /// The next captured frame with its capture time: nanoseconds on the Core
    /// Audio host clock (see [`host_time_now_ns`]) of the frame's first sample,
    /// 0 if the system gave none.
    pub fn try_recv_stamped(&mut self) -> Option<(AudioFrame, u64)> {
        let meta = self.consumer.pop_into(&mut self.scratch)?;
        let mut data = Vec::with_capacity(self.scratch.len() * std::mem::size_of::<f32>());
        for sample in &self.scratch {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let frame = AudioFrame {
            sequence_no: self.sequence,
            sample_rate: meta.sample_rate,
            channels: meta.channels as u16,
            format: AudioFormat::F32,
            timestamp_ms: 0,
            data,
        };
        self.sequence += 1;
        Some((frame, meta.host_time))
    }

    /// Wait up to `timeout` for the next frame.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<AudioFrame> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(frame) = self.try_recv() {
                return Some(frame);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Peak absolute amplitude across a float32 [`AudioFrame`]'s samples, in `0.0..`.
///
/// Handy for the "is the captured stream actually non-silent?" check
/// (some apps can deliver a silent tap stream).
pub fn frame_peak_amplitude(frame: &AudioFrame) -> f32 {
    if frame.format != AudioFormat::F32 {
        return 0.0;
    }
    let mut peak = 0.0f32;
    for bytes in frame.data.as_chunks::<4>().0 {
        let sample = f32::from_le_bytes(*bytes).abs();
        if sample > peak {
            peak = sample;
        }
    }
    peak
}

/// Convenience alias for sharing a tap handle across owners during validation.
pub type SharedTap = Arc<CoreAudioTap>;

// ---------------------------------------------------------------------------
// AudioSource implementation
// ---------------------------------------------------------------------------

use std::time::Instant;

use prxvc_media::{AudioFrame as MediaFrame, MonoTime, SampleRate, SystemClock};
use prxvc_ports::{AudioSource, EndpointError, EndpointHealth, SourceInfo};

use crate::resample::downmix_to_mono;
use crate::routing::ProcessFilter;
use crate::tap_host_time::{FrameStamper, HostTimeMap};

/// The rate reported by [`AudioSource::start`] before the first frame: the
/// tap's native rate is 48 kHz on current hardware. Each frame carries the
/// rate it was really captured at.
const NOMINAL_RATE: SampleRate = SampleRate::HZ_48000;

/// macOS [`AudioSource`] backed by a Core Audio process tap.
///
/// Lazily starts the tap on [`start`](AudioSource::start) using the configured
/// [`TapConfig`], delivers frames downmixed to mono at the tap's native rate via
/// [`read`](AudioSource::read), and tears the tap down on
/// [`stop`](AudioSource::stop) or drop. Each frame is stamped with its capture
/// time: the host time of its IO cycle, converted to the pipeline clock by
/// [`FrameStamper`] (a frame whose capture time is missing or implausible gets
/// the read time instead and is counted).
pub struct CoreAudioTapSource {
    config: TapConfig,
    incidents: Arc<TapIncidents>,
    inner: Option<(CoreAudioTap, TapReceiver)>,
    clock: SystemClock,
    stamper: Option<FrameStamper>,
    sequence: u64,
    overruns_seen: u64,
    frames: u64,
    last_error: Option<String>,
}

impl CoreAudioTapSource {
    /// Create an unmuted source that will capture the given process filter when started.
    pub fn new(filter: ProcessFilter) -> Self {
        let config = match filter {
            ProcessFilter::IncludeApp(pid) => TapConfig::for_pid(pid),
            ProcessFilter::IncludeBundle(id) => TapConfig::for_bundle(id),
        };
        Self::with_config(config)
    }

    /// Create a source with an explicit tap configuration (target and mute mode).
    pub fn with_config(config: TapConfig) -> Self {
        Self {
            config,
            incidents: Arc::default(),
            inner: None,
            clock: SystemClock::new(),
            stamper: None,
            sequence: 0,
            overruns_seen: 0,
            frames: 0,
            last_error: None,
        }
    }

    /// The counters of re-creations, shared with whoever reads them while the source runs
    /// on another thread.
    pub fn incidents(&self) -> Arc<TapIncidents> {
        Arc::clone(&self.incidents)
    }

    /// Health events of the running tap, if any.
    pub fn try_recv_event(&self) -> Option<TapEvent> {
        self.inner.as_ref().and_then(|(tap, _)| tap.events().try_recv().ok())
    }

    /// Counters of the running tap, if any.
    pub fn stats(&self) -> Option<TapStats> {
        self.inner.as_ref().map(|(tap, _)| tap.stats())
    }

    /// Frames stamped with the read time because their capture time was
    /// missing or implausible.
    pub fn stamp_fallbacks(&self) -> u64 {
        self.stamper.as_ref().map_or(0, FrameStamper::fallbacks)
    }

    /// Downmix a tap frame to a mono media frame stamped with its capture time.
    fn frame_to_media(&mut self, frame: &AudioFrame, host_ns: u64, gap: bool) -> Result<MediaFrame, EndpointError> {
        let mono = downmix_to_mono(frame).map_err(|e| EndpointError::Internal(e.to_string()))?;
        let rate = SampleRate::new(frame.sample_rate)
            .map_err(|e| EndpointError::Internal(e.to_string()))?;
        let now: MonoTime = prxvc_media::Clock::now(&self.clock);
        let ts = self.stamper.as_mut().map_or(now, |s| s.stamp(host_ns, now));
        let media = MediaFrame::new(mono, rate, ts, self.sequence)
            .map_err(|e| EndpointError::Internal(e.to_string()))?;
        self.sequence += 1;
        Ok(media.with_discontinuity(gap))
    }
}

impl AudioSource for CoreAudioTapSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        if self.inner.is_some() {
            return Err(EndpointError::Internal("tap is already started".to_owned()));
        }
        match CoreAudioTap::start_counted(self.config.clone(), Arc::clone(&self.incidents)) {
            Ok(inner) => {
                self.inner = Some(inner);
                // One calibration pair of the host clock and the pipeline clock.
                let host_now = host_time_now_ns();
                let mono_now = prxvc_media::Clock::now(&self.clock);
                self.stamper = Some(FrameStamper::new(HostTimeMap::new(host_now, mono_now)));
                self.sequence = 0;
                self.overruns_seen = 0;
                self.last_error = None;
                Ok(SourceInfo { rate: NOMINAL_RATE, device: "process tap".to_owned() })
            }
            Err(e) => {
                let e = EndpointError::Unavailable { reason: e.to_string() };
                self.last_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    fn read(&mut self, out: &mut Vec<MediaFrame>, max_wait: Duration) -> Result<(), EndpointError> {
        let deadline = Instant::now() + max_wait;
        loop {
            let Some((tap, rx)) = self.inner.as_mut() else {
                return Err(EndpointError::NotStarted);
            };
            let overruns = tap.stats().overrun_slots;
            let mut gap = overruns != self.overruns_seen;
            self.overruns_seen = overruns;
            let mut taken = 0;
            let mut raw = Vec::new();
            while let Some(stamped) = rx.try_recv_stamped() {
                raw.push(stamped);
            }
            for (frame, host_ns) in &raw {
                let media = self.frame_to_media(frame, *host_ns, std::mem::take(&mut gap))?;
                out.push(media);
                taken += 1;
            }
            self.frames += taken;
            if taken > 0 || Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(2).min(deadline - Instant::now()));
        }
    }

    fn health(&self) -> EndpointHealth {
        let stats = self.stats();
        EndpointHealth {
            running: self.inner.is_some(),
            frames: self.frames,
            dropped_samples: stats.map_or(0, |s| s.overrun_frames),
            underruns: 0,
            last_error: self.last_error.clone(),
        }
    }

    fn stop(&mut self) {
        // Dropping the CoreAudioTap stops the native tap.
        self.inner = None;
    }
}

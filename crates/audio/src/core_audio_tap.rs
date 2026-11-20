//! Core Audio process-tap capture (macOS 14.4+): safe wrapper over `native/prxvc_tap.m` that delivers one application's audio as frames.
//!
//! The IO callback ([`on_buffers`]) runs on the Core Audio HAL thread, a
//! real-time thread: it only validates the buffer shape, copies samples into a
//! preallocated lock-free ring ([`crate::tap_ring`]) and bumps atomic counters.
//! No allocation, lock or logging happens there; a full ring drops the new data
//! and counts the overrun. Everything else (frame building, health checks,
//! re-creation) runs on normal threads; see [`crate::tap_supervisor`].
//!
//! Frames are emitted as raw interleaved **float32 at the tap's native rate**
//! (typically 48 kHz stereo); downmix-to-mono and resampling to 16 kHz for ASR
//! happen downstream.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::tap_format::{InputBuffer, MAX_BUFFERS, RawFormat, StreamLayout};
use crate::tap_ring::{RingConsumer, RingProducer, SLOT_SAMPLES, tap_ring};
use crate::tap_supervisor::{Supervisor, spawn_supervisor};
use crate::tap_types::{TapConfig, TapCounters, TapError, TapEvent, TapStats, TapTarget};
use crate::{AudioError, AudioFormat, AudioFrame};

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
struct RawTapBuffer {
    channels: u32,
    byte_size: u32,
    data: *const c_void,
}

type TapAudioCb = extern "C" fn(ctx: *mut c_void, count: u32, buffers: *const RawTapBuffer, host_time: u64);

const ERR_UNSUPPORTED_OS: i32 = 1;
const ERR_PROCESS_NOT_FOUND: i32 = 2;
const ERR_CORE_AUDIO: i32 = 3;
const PERMISSION_DENIED: i32 = 1;

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
    fn prxvc_tap_permission_status() -> i32;
}

// ---------------------------------------------------------------------------
// Callback context and the real-time callback
// ---------------------------------------------------------------------------

/// State handed to the C callback as its opaque `ctx`. Lives in an [`Arc`] for
/// the whole supervision, so it outlives every native tap that uses it.
pub(crate) struct TapCtx {
    /// Packed [`StreamLayout`] of the current native tap; `0` while none is installed.
    layout: AtomicU64,
    /// Written only by the IO callback, which the HAL calls serially, and only
    /// while a native tap is running; see [`on_buffers`].
    producer: std::cell::UnsafeCell<RingProducer>,
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

/// IO callback. Runs on the Core Audio HAL thread: nothing here may allocate,
/// lock, log or block (CS-4.1.1).
extern "C" fn on_buffers(ctx: *mut c_void, count: u32, buffers: *const RawTapBuffer, host_time: u64) {
    if ctx.is_null() || buffers.is_null() || count == 0 {
        return;
    }
    // A panic must never unwind into C; `catch_unwind` does not allocate unless
    // a panic occurs.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `ctx` is `Arc::as_ptr` of a `TapCtx` that outlives the native
        // tap (the supervisor drops the tap before the last `Arc`).
        let ctx = unsafe { &*(ctx as *const TapCtx) };
        // SAFETY: the shim passes `count` valid `RawTapBuffer`s for the call's duration.
        let raw = unsafe { std::slice::from_raw_parts(buffers, (count as usize).min(MAX_BUFFERS)) };
        // SAFETY: serial callbacks, see `TapCtx::producer`.
        let producer = unsafe { &mut *ctx.producer.get() };
        deliver(ctx, producer, raw, host_time);
    }));
}

/// Validate one callback's buffers against the installed layout and queue them.
fn deliver(ctx: &TapCtx, producer: &mut RingProducer, raw: &[RawTapBuffer], host_time: u64) {
    let counters = &ctx.counters;
    // Counted first so a stream of unreadable buffers is not mistaken for a stall.
    counters.callbacks.fetch_add(1, Ordering::Relaxed);
    let Some(layout) = StreamLayout::unpack(ctx.layout.load(Ordering::Acquire)) else {
        return;
    };
    let mut views = [InputBuffer::EMPTY; MAX_BUFFERS];
    for (view, b) in views.iter_mut().zip(raw) {
        if b.data.is_null() || !(b.data as usize).is_multiple_of(std::mem::align_of::<f32>()) || !b.byte_size.is_multiple_of(4) {
            counters.bad_buffers.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // SAFETY: non-null, 4-byte aligned, `byte_size` bytes valid for this call.
        let samples = unsafe { std::slice::from_raw_parts(b.data as *const f32, (b.byte_size / 4) as usize) };
        *view = InputBuffer { channels: b.channels, samples };
    }
    let views = &views[..raw.len()];
    let Some(frames) = layout.frames_in(views) else {
        counters.bad_buffers.fetch_add(1, Ordering::Relaxed);
        return;
    };
    counters.frames.fetch_add(frames as u64, Ordering::Relaxed);

    let channels = layout.channels as usize;
    let per_slot = SLOT_SAMPLES / channels;
    let mut start = 0;
    while start < frames {
        let take = per_slot.min(frames - start);
        let queued = producer.push_with(layout.channels, layout.sample_rate, host_time, |out| {
            layout.interleave_into(views, start, take, out);
            take * channels
        });
        if !queued {
            // Ring full: the consumer fell behind. Drop the rest and count it.
            counters.overrun_slots.fetch_add(1, Ordering::Relaxed);
            counters.overrun_frames.fetch_add((frames - start) as u64, Ordering::Relaxed);
            return;
        }
        start += take;
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
    pub(crate) fn open(config: &TapConfig, ctx: &Arc<TapCtx>) -> Result<Self, TapError> {
        if permission_status() == PermissionStatus::Denied {
            return Err(TapError::PermissionDenied);
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

/// The user's answer for system audio recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    /// Allowed.
    Granted,
    /// Refused; the tap would deliver silence.
    Denied,
    /// Not asked yet; creating the tap shows the system prompt.
    Undetermined,
    /// The system gives no answer.
    Unknown,
}

/// Ask the system whether system audio recording is allowed. Never prompts.
pub fn permission_status() -> PermissionStatus {
    // SAFETY: no arguments; the shim only reads the permission database.
    match unsafe { prxvc_tap_permission_status() } {
        0 => PermissionStatus::Granted,
        PERMISSION_DENIED => PermissionStatus::Denied,
        2 => PermissionStatus::Undetermined,
        _ => PermissionStatus::Unknown,
    }
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
    /// # Errors
    /// A [`TapError`]: unsupported OS, permission denied (with the text a user
    /// needs), target not running, unsupported stream format, or a Core Audio failure.
    pub fn start(config: TapConfig) -> Result<(Self, TapReceiver), TapError> {
        let (producer, consumer) = tap_ring(RING_SLOTS);
        let ctx = Arc::new(TapCtx::new(producer));
        let native = NativeTap::open(&config, &ctx)?;
        let counters = Arc::clone(&ctx.counters);
        let (events_tx, events) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = spawn_supervisor(Supervisor {
            config,
            ctx,
            native: Some(native),
            events: events_tx,
            stop: Arc::clone(&stop),
        })?;
        Ok((Self { counters, events, stop, thread: Some(thread) }, TapReceiver::new(consumer)))
    }

    /// Health events (stall, target gone, restart) since the last call.
    pub fn events(&self) -> &Receiver<TapEvent> {
        &self.events
    }

    /// Callback, frame and overrun counters.
    pub fn stats(&self) -> TapStats {
        self.counters.snapshot()
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
        Some(frame)
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
    for chunk in frame.data.chunks_exact(std::mem::size_of::<f32>()) {
        // chunks_exact yields exactly 4 bytes; build a fixed array for from_le_bytes.
        let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let sample = f32::from_le_bytes(bytes).abs();
        if sample > peak {
            peak = sample;
        }
    }
    peak
}

/// Convenience alias for sharing a tap handle across owners during validation.
pub type SharedTap = Arc<CoreAudioTap>;

// ---------------------------------------------------------------------------
// AppAudioSource implementation
// ---------------------------------------------------------------------------

use crate::AudioState;
use crate::routing::{AppAudioSource, ProcessFilter};

/// macOS [`AppAudioSource`] backed by a Core Audio process tap.
///
/// Lazily starts the tap on [`start`](AppAudioSource::start) using the configured
/// [`TapConfig`], delivers native-format frames via
/// [`try_recv`](AppAudioSource::try_recv), and tears the tap down on
/// [`stop`](AppAudioSource::stop) or drop.
pub struct CoreAudioTapSource {
    config: TapConfig,
    inner: Option<(CoreAudioTap, TapReceiver)>,
    state: AudioState,
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
        Self { config, inner: None, state: AudioState::InputClosed }
    }

    /// Health events of the running tap, if any.
    pub fn try_recv_event(&self) -> Option<TapEvent> {
        self.inner.as_ref().and_then(|(tap, _)| tap.events().try_recv().ok())
    }

    /// Counters of the running tap, if any.
    pub fn stats(&self) -> Option<TapStats> {
        self.inner.as_ref().map(|(tap, _)| tap.stats())
    }
}

impl AppAudioSource for CoreAudioTapSource {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.inner.is_some() {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: AudioState::InputStreaming.to_string(),
            });
        }
        self.inner = Some(CoreAudioTap::start(self.config.clone())?);
        self.state = AudioState::InputStreaming;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        // Dropping the CoreAudioTap stops the native tap.
        self.inner = None;
        self.state = AudioState::InputClosed;
        Ok(())
    }

    fn try_recv(&mut self) -> Option<AudioFrame> {
        self.inner.as_mut().and_then(|(_, rx)| rx.try_recv())
    }

    fn state(&self) -> AudioState {
        self.state
    }
}

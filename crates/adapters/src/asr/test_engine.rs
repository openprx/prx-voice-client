//! A scripted engine for the adapter's tests: records its calls, can be held, and answers text
//! computed from the audio length or taken from a script.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use prxvc_media::Lang;

use super::seam::{EngineError, RecognizeEngine};

type TextFn = Box<dyn Fn(Lang, usize) -> String + Send>;

struct Shared {
    calls: Mutex<Vec<(Lang, usize)>>,
    started: AtomicUsize,
    aborted: AtomicUsize,
    finished: AtomicUsize,
    hold: Mutex<bool>,
    released: Condvar,
    text: Mutex<TextFn>,
    script: Mutex<VecDeque<String>>,
    fail: AtomicBool,
    panic_next: AtomicBool,
}

/// The engine handed to `QwenAsr::start`.
pub(crate) struct TestEngine {
    shared: Arc<Shared>,
}

/// The test's side: inspect and steer the engine.
#[derive(Clone)]
pub(crate) struct TestHandle {
    shared: Arc<Shared>,
}

/// A new engine and its handle. By default the text is `"len<samples>"`.
pub(crate) fn engine() -> (TestEngine, TestHandle) {
    let shared = Arc::new(Shared {
        calls: Mutex::new(Vec::new()),
        started: AtomicUsize::new(0),
        aborted: AtomicUsize::new(0),
        finished: AtomicUsize::new(0),
        hold: Mutex::new(false),
        released: Condvar::new(),
        text: Mutex::new(Box::new(|_, n| format!("len{n}"))),
        script: Mutex::new(VecDeque::new()),
        fail: AtomicBool::new(false),
        panic_next: AtomicBool::new(false),
    });
    (
        TestEngine {
            shared: Arc::clone(&shared),
        },
        TestHandle { shared },
    )
}

impl RecognizeEngine for TestEngine {
    fn decode(
        &mut self,
        audio: &[f32],
        lang: Lang,
        stop: &dyn Fn() -> bool,
    ) -> Result<String, EngineError> {
        let s = &self.shared;
        s.calls.lock().push((lang, audio.len()));
        s.started.fetch_add(1, Ordering::SeqCst);
        if s.panic_next.swap(false, Ordering::SeqCst) {
            panic!("scripted engine panic");
        }
        let mut hold = s.hold.lock();
        while *hold {
            if stop() {
                s.aborted.fetch_add(1, Ordering::SeqCst);
                return Err(EngineError::Aborted);
            }
            s.released.wait_for(&mut hold, Duration::from_millis(1));
        }
        drop(hold);
        if s.fail.load(Ordering::SeqCst) {
            s.finished.fetch_add(1, Ordering::SeqCst);
            return Err(EngineError::Failed("scripted failure".to_owned()));
        }
        let text = s
            .script
            .lock()
            .pop_front()
            .unwrap_or_else(|| (s.text.lock())(lang, audio.len()));
        s.finished.fetch_add(1, Ordering::SeqCst);
        Ok(text)
    }
}

impl TestHandle {
    /// Calls so far as `(language, samples)`, including the warm-up calls.
    pub(crate) fn calls(&self) -> Vec<(Lang, usize)> {
        self.shared.calls.lock().clone()
    }

    pub(crate) fn started(&self) -> usize {
        self.shared.started.load(Ordering::SeqCst)
    }

    pub(crate) fn finished(&self) -> usize {
        self.shared.finished.load(Ordering::SeqCst)
    }

    pub(crate) fn aborted(&self) -> usize {
        self.shared.aborted.load(Ordering::SeqCst)
    }

    /// Block every decode until [`TestHandle::release`] (an abort still gets through).
    pub(crate) fn hold(&self) {
        *self.shared.hold.lock() = true;
    }

    pub(crate) fn release(&self) {
        *self.shared.hold.lock() = false;
        self.shared.released.notify_all();
    }

    /// Answer these texts, one per decode, before falling back to the default.
    pub(crate) fn script(&self, texts: &[&str]) {
        self.shared
            .script
            .lock()
            .extend(texts.iter().map(|t| (*t).to_owned()));
    }

    /// Replace the default text function.
    pub(crate) fn text_with(&self, f: impl Fn(Lang, usize) -> String + Send + 'static) {
        *self.shared.text.lock() = Box::new(f);
    }

    pub(crate) fn fail_decodes(&self, fail: bool) {
        self.shared.fail.store(fail, Ordering::SeqCst);
    }

    pub(crate) fn panic_on_next_decode(&self) {
        self.shared.panic_next.store(true, Ordering::SeqCst);
    }
}

//! The panic hook logs the thread name, the location and the message of a panic on a worker
//! thread. The hook is process-wide, so this lives in its own test binary.

use std::io;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("capture").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;

    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

#[test]
fn a_worker_thread_panic_is_logged_with_thread_location_and_message() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("subscriber");
    prxvc_observe::panic_report::install();

    let handle = std::thread::Builder::new()
        .name("prxvc-pump-test".to_owned())
        .spawn(|| {
            let held: Vec<u8> = Vec::new();
            let at = held.len() + 3;
            let _ = held[at];
        })
        .expect("spawn");
    assert!(handle.join().is_err());

    let text = String::from_utf8(capture.0.lock().expect("capture").clone()).expect("utf8");
    assert!(text.contains("thread panicked"), "{text}");
    assert!(text.contains("prxvc-pump-test"), "{text}");
    assert!(text.contains("panic_hook.rs"), "{text}");
    assert!(text.contains("index out of bounds"), "{text}");
}

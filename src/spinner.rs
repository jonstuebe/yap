use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FRAMES: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
];

pub struct Spinner {
    label: String,
    stop: Arc<AtomicBool>,
    start: Instant,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    pub fn start(label: impl Into<String>) -> Self {
        let label = label.into();
        let stop = Arc::new(AtomicBool::new(false));
        let start = Instant::now();
        let handle = if std::io::stderr().is_terminal() {
            let stop = stop.clone();
            let label = label.clone();
            Some(thread::spawn(move || {
                let mut i = 0;
                while !stop.load(Ordering::SeqCst) {
                    let elapsed = start.elapsed().as_secs_f32();
                    let mut err = std::io::stderr().lock();
                    let _ = write!(err, "\r{} {} {:4.1}s", FRAMES[i % FRAMES.len()], label, elapsed);
                    let _ = err.flush();
                    drop(err);
                    i += 1;
                    thread::sleep(Duration::from_millis(80));
                }
            }))
        } else {
            None
        };
        Self {
            label,
            stop,
            start,
            handle,
        }
    }

    pub fn finish(mut self, ok: bool) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            h.join().ok();
            let mark = if ok { "✓" } else { "⏹" };
            let elapsed = self.start.elapsed().as_secs_f32();
            let mut err = std::io::stderr().lock();
            let _ = writeln!(err, "\r\x1b[K{} {} {:.1}s", mark, self.label, elapsed);
            let _ = err.flush();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            h.join().ok();
        }
    }
}

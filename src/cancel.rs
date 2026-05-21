use anyhow::{Context, Result};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);
static HOOKS: Mutex<Vec<Box<dyn Fn() + Send + 'static>>> = Mutex::new(Vec::new());

pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

pub fn on_cancel<F: Fn() + Send + 'static>(f: F) {
    HOOKS.lock().unwrap().push(Box::new(f));
}

pub fn install() -> Result<()> {
    ctrlc::set_handler(|| {
        CANCELLED.store(true, Ordering::SeqCst);
        let hooks = match HOOKS.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if hooks.is_empty() {
            std::process::exit(130);
        }
        for h in hooks.iter() {
            h();
        }
    })
    .context("installing ctrl-c handler")?;
    Ok(())
}

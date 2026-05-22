mod assets;
mod cancel;
mod chunk;
mod clipboard;
mod play;
mod spinner;
mod synth;
mod update;

use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use crossbeam_channel::bounded;
use rodio::Sink;
use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(about = "Speak clipboard text with Kokoro TTS.", version)]
struct Args {
    #[arg(long, default_value = "af_heart")]
    voice: String,
    #[arg(long, default_value_t = 1.0)]
    speed: f32,
    #[arg(long, default_value = "en-us")]
    lang: String,
    #[arg(long)]
    list_voices: bool,
    /// Watch the clipboard and speak each time it changes. Ctrl-C to quit.
    #[arg(long)]
    watch: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Update yap to the latest release.
    Update,
}

fn main() -> Result<()> {
    cancel::install()?;
    let args = Args::parse();

    if matches!(args.command, Some(Command::Update)) {
        return update::run();
    }

    if let Err(e) = assets::ensure() {
        if cancel::cancelled() {
            std::process::exit(130);
        }
        return Err(e);
    }

    // espeak-rs hardcodes the build-time data path; point it at our cache
    // so it works on machines other than the one that built the binary.
    // Don't override if the user has set their own (e.g. system espeak-ng).
    if std::env::var_os("PIPER_ESPEAKNG_DATA_DIRECTORY").is_none()
        && let Ok(parent) = assets::espeak_data_parent()
    {
        // SAFETY: called before any threads are spawned.
        unsafe { std::env::set_var("PIPER_ESPEAKNG_DATA_DIRECTORY", parent) };
    }

    let synth = synth::Synth::load()?;

    if args.list_voices {
        let mut voices = synth.voices();
        voices.sort();
        for v in voices {
            println!("{v}");
        }
        return Ok(());
    }

    if args.watch {
        watch(synth, args.voice, args.speed, args.lang)?;
        if cancel::cancelled() {
            std::process::exit(130);
        }
        return Ok(());
    }

    let text = clipboard::read()?;
    let chunks = chunk::chunk_text(&text);
    if chunks.is_empty() {
        eprintln!("clipboard is empty");
        std::process::exit(1);
    }

    let cancelled = run(synth, chunks, args)?;
    if cancelled {
        std::process::exit(130);
    }
    Ok(())
}

fn run(synth: synth::Synth, chunks: Vec<String>, args: Args) -> Result<bool> {
    let total = chunks.len();
    let player = play::Player::new()?;
    let (tx, rx) = bounded::<(usize, Vec<f32>)>(2);

    {
        let sink = player.sink();
        cancel::on_cancel(move || sink.stop());
    }

    let producer = {
        let voice = args.voice.clone();
        let lang = args.lang.clone();
        let speed = args.speed;
        thread::spawn(move || -> Result<()> {
            for (i, chunk) in chunks.into_iter().enumerate() {
                if cancel::cancelled() {
                    break;
                }
                let samples = synth.synthesize(&chunk, &voice, speed, &lang)?;
                if cancel::cancelled() {
                    break;
                }
                let mut pending = Some((i, samples));
                while !cancel::cancelled() {
                    match tx.send_timeout(pending.take().unwrap(), Duration::from_millis(200)) {
                        Ok(()) => break,
                        Err(crossbeam_channel::SendTimeoutError::Timeout(item)) => {
                            pending = Some(item);
                        }
                        Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => {
                            return Ok(());
                        }
                    }
                }
            }
            Ok(())
        })
    };

    let tty = std::io::stderr().is_terminal();

    let spin = spinner::Spinner::start(format!("synthesizing first of {total} chunk(s)"));
    let first = rx.recv();
    spin.finish(first.is_ok());

    let mut item = match first {
        Ok(v) => Some(v),
        Err(_) => None,
    };

    while let Some((i, samples)) = item.take() {
        if cancel::cancelled() {
            break;
        }
        if tty {
            let mut err = std::io::stderr().lock();
            let _ = write!(err, "\r\x1b[K▶ playing {}/{}", i + 1, total);
            let _ = err.flush();
        }
        player.play_blocking(samples, synth::SAMPLE_RATE);
        if cancel::cancelled() {
            break;
        }
        item = rx.recv().ok();
    }

    if tty {
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "\r\x1b[K");
        let _ = err.flush();
    }

    while rx.try_recv().is_ok() {}
    drop(rx);
    if let Err(e) = producer.join().unwrap_or(Ok(())) {
        return Err(e);
    }

    Ok(cancel::cancelled())
}

struct WatchJob {
    stop: Arc<AtomicBool>,
    sink: Arc<Sink>,
    handle: JoinHandle<Result<synth::Synth>>,
}

impl WatchJob {
    fn stop_and_join(self) -> Result<synth::Synth> {
        self.stop.store(true, Ordering::SeqCst);
        self.sink.stop();
        self.handle
            .join()
            .unwrap_or_else(|_| Err(anyhow!("watch job thread panicked")))
    }
}

fn watch(synth: synth::Synth, voice: String, speed: f32, lang: String) -> Result<()> {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => return Err(anyhow!("opening clipboard: {e}")),
    };
    let mut last = clipboard.get_text().unwrap_or_default();

    let current_sink: Arc<Mutex<Option<Arc<Sink>>>> = Arc::new(Mutex::new(None));
    {
        let cs = current_sink.clone();
        cancel::on_cancel(move || {
            if let Some(s) = cs.lock().unwrap().as_ref() {
                s.stop();
            }
        });
    }

    let tty = std::io::stderr().is_terminal();
    eprintln!("yap --watch: copy text to speak; Ctrl-C to quit");

    let mut synth_slot: Option<synth::Synth> = Some(synth);
    let mut current: Option<WatchJob> = None;

    while !cancel::cancelled() {
        match clipboard.get_text() {
            Ok(text) if text != last && !text.trim().is_empty() => {
                last = text.clone();

                if let Some(prev) = current.take() {
                    *current_sink.lock().unwrap() = None;
                    synth_slot = Some(prev.stop_and_join()?);
                }

                let chunks = chunk::chunk_text(&text);
                if chunks.is_empty() {
                    continue;
                }

                let s = synth_slot
                    .take()
                    .ok_or_else(|| anyhow!("synth missing between jobs"))?;
                let job = spawn_watch_job(s, chunks, voice.clone(), speed, lang.clone(), tty)?;
                *current_sink.lock().unwrap() = Some(job.sink.clone());
                current = Some(job);
            }
            Ok(text) => {
                last = text;
            }
            Err(_) => {}
        }

        // If the current job finished on its own, reap it so we're idle again.
        if let Some(j) = current.as_ref()
            && j.handle.is_finished()
        {
            let j = current.take().unwrap();
            *current_sink.lock().unwrap() = None;
            synth_slot = Some(j.stop_and_join()?);
        }

        thread::sleep(Duration::from_millis(200));
    }

    if let Some(j) = current.take() {
        *current_sink.lock().unwrap() = None;
        let _ = j.stop_and_join();
    }

    Ok(())
}

fn spawn_watch_job(
    synth: synth::Synth,
    chunks: Vec<String>,
    voice: String,
    speed: f32,
    lang: String,
    tty: bool,
) -> Result<WatchJob> {
    // Player owns a cpal::Stream which is not Send on macOS, so it has to be
    // created on the job thread itself. We hand its sink back via a channel.
    let stop = Arc::new(AtomicBool::new(false));
    let (init_tx, init_rx) = bounded::<Result<Arc<Sink>>>(1);
    let stop_t = stop.clone();

    let handle = thread::spawn(move || -> Result<synth::Synth> {
        let player = match play::Player::new() {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("{e}");
                let _ = init_tx.send(Err(e));
                return Err(anyhow!("audio player init failed: {msg}"));
            }
        };
        if init_tx.send(Ok(player.sink())).is_err() {
            return Ok(synth);
        }
        drop(init_tx);
        run_watch_job(&synth, &player, chunks, &voice, speed, &lang, &stop_t, tty)?;
        Ok(synth)
    });

    let sink = match init_rx.recv() {
        Ok(res) => res?,
        Err(_) => {
            return Err(match handle.join() {
                Ok(Err(e)) => e,
                Ok(Ok(_)) => anyhow!("watch job exited before init"),
                Err(_) => anyhow!("watch job thread panicked during init"),
            });
        }
    };

    Ok(WatchJob { stop, sink, handle })
}

fn run_watch_job(
    synth: &synth::Synth,
    player: &play::Player,
    chunks: Vec<String>,
    voice: &str,
    speed: f32,
    lang: &str,
    stop: &AtomicBool,
    tty: bool,
) -> Result<()> {
    fn stopped(stop: &AtomicBool) -> bool {
        stop.load(Ordering::SeqCst) || cancel::cancelled()
    }

    let total = chunks.len();

    let result = thread::scope(|s| -> Result<()> {
        let (tx, rx) = bounded::<(usize, Vec<f32>)>(2);

        let producer = s.spawn(move || -> Result<()> {
            for (i, chunk) in chunks.into_iter().enumerate() {
                if stopped(stop) {
                    break;
                }
                let samples = synth.synthesize(&chunk, voice, speed, lang)?;
                if stopped(stop) {
                    break;
                }
                let mut pending = Some((i, samples));
                while !stopped(stop) {
                    match tx.send_timeout(pending.take().unwrap(), Duration::from_millis(200)) {
                        Ok(()) => break,
                        Err(crossbeam_channel::SendTimeoutError::Timeout(item)) => {
                            pending = Some(item);
                        }
                        Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => {
                            return Ok(());
                        }
                    }
                }
            }
            Ok(())
        });

        loop {
            if stopped(stop) {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok((i, samples)) => {
                    if stopped(stop) {
                        break;
                    }
                    if tty {
                        let mut err = std::io::stderr().lock();
                        let _ = write!(err, "\r\x1b[K▶ speaking {}/{}", i + 1, total);
                        let _ = err.flush();
                    }
                    player.play_blocking(samples, synth::SAMPLE_RATE);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }

        while rx.try_recv().is_ok() {}
        drop(rx);
        producer.join().unwrap_or(Ok(()))
    });

    if tty {
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "\r\x1b[K");
        let _ = err.flush();
    }

    result
}

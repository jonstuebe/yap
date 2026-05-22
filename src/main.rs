mod assets;
mod cancel;
mod chunk;
mod clipboard;
mod play;
mod save;
mod spinner;
mod synth;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossbeam_channel::bounded;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::thread;
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
    /// Write audio to a file (.wav or .mp3) instead of playing it.
    #[arg(long, value_name = "PATH")]
    save: Option<PathBuf>,

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

    let text = clipboard::read()?;
    let chunks = chunk::chunk_text(&text);
    if chunks.is_empty() {
        eprintln!("clipboard is empty");
        std::process::exit(1);
    }

    let cancelled = if let Some(path) = args.save.clone() {
        run_save(synth, chunks, args, path)?
    } else {
        run_play(synth, chunks, args)?
    };
    if cancelled {
        std::process::exit(130);
    }
    Ok(())
}

fn spawn_producer(
    synth: synth::Synth,
    chunks: Vec<String>,
    args: &Args,
    tx: crossbeam_channel::Sender<(usize, Vec<f32>)>,
) -> thread::JoinHandle<Result<()>> {
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
}

fn run_play(synth: synth::Synth, chunks: Vec<String>, args: Args) -> Result<bool> {
    let total = chunks.len();
    let player = play::Player::new()?;
    let (tx, rx) = bounded::<(usize, Vec<f32>)>(2);

    {
        let sink = player.sink();
        cancel::on_cancel(move || sink.stop());
    }

    let producer = spawn_producer(synth, chunks, &args, tx);

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

fn run_save(
    synth: synth::Synth,
    chunks: Vec<String>,
    args: Args,
    path: PathBuf,
) -> Result<bool> {
    let total = chunks.len();
    let mut sink = save::Sink::create(&path, synth::SAMPLE_RATE)?;
    let (tx, rx) = bounded::<(usize, Vec<f32>)>(2);

    let producer = spawn_producer(synth, chunks, &args, tx);

    let tty = std::io::stderr().is_terminal();

    let spin = spinner::Spinner::start(format!("synthesizing first of {total} chunk(s)"));
    let first = rx.recv();
    spin.finish(first.is_ok());

    let mut item = first.ok();
    let mut write_err: Option<anyhow::Error> = None;

    while let Some((i, samples)) = item.take() {
        if cancel::cancelled() {
            break;
        }
        if tty {
            let mut err = std::io::stderr().lock();
            let _ = write!(err, "\r\x1b[K💾 writing {}/{}", i + 1, total);
            let _ = err.flush();
        }
        if let Err(e) = sink.write(&samples) {
            write_err = Some(e);
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
    let producer_result = producer.join().unwrap_or(Ok(()));

    if let Some(e) = write_err {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    if let Err(e) = producer_result {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    if cancel::cancelled() {
        let _ = std::fs::remove_file(&path);
        return Ok(true);
    }

    sink.finalize()?;
    eprintln!("saved {}", path.display());
    Ok(false)
}

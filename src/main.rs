mod assets;
mod cancel;
mod chunk;
mod clipboard;
mod play;
mod spinner;
mod synth;

use anyhow::Result;
use clap::Parser;
use crossbeam_channel::bounded;
use std::io::{IsTerminal, Write};
use std::thread;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(about = "Speak clipboard text with Kokoro TTS.")]
struct Args {
    #[arg(long, default_value = "af_heart")]
    voice: String,
    #[arg(long, default_value_t = 1.0)]
    speed: f32,
    #[arg(long, default_value = "en-us")]
    lang: String,
    #[arg(long)]
    list_voices: bool,
}

fn main() -> Result<()> {
    cancel::install()?;
    let args = Args::parse();

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

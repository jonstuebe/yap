use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cancel;

const MODEL_URL: &str = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx";
const VOICES_URL: &str = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin";

pub fn paths() -> Result<(PathBuf, PathBuf)> {
    let base = dirs::data_dir()
        .context("could not locate user data dir")?
        .join("yap");
    std::fs::create_dir_all(&base).with_context(|| format!("creating {}", base.display()))?;
    Ok((base.join("kokoro-v1.0.onnx"), base.join("voices-v1.0.bin")))
}

pub fn ensure() -> Result<()> {
    let (model_path, voices_path) = paths()?;
    let mut missing: Vec<(&'static str, &'static str, PathBuf)> = Vec::new();
    if !model_path.exists() {
        missing.push(("kokoro-v1.0.onnx", MODEL_URL, model_path));
    }
    if !voices_path.exists() {
        missing.push(("voices-v1.0.bin", VOICES_URL, voices_path));
    }
    if missing.is_empty() {
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("building http client")?;

    let mut assets: Vec<(&'static str, &'static str, PathBuf, Option<u64>)> =
        Vec::with_capacity(missing.len());
    for (name, url, path) in missing {
        let size = client
            .head(url)
            .send()
            .with_context(|| format!("HEAD {url}"))?
            .content_length();
        assets.push((name, url, path, size));
    }

    let total_known: u64 = assets.iter().filter_map(|(_, _, _, s)| *s).sum();

    println!("yap needs to download the Kokoro TTS model on first run:");
    for (name, _, path, size) in &assets {
        let size_str = size.map(human_bytes).unwrap_or_else(|| "?".into());
        println!("  • {name} ({size_str}) → {}", path.display());
    }
    if total_known > 0 {
        println!("  total: {} on disk", human_bytes(total_known));
    }

    if !io::stdin().is_terminal() {
        anyhow::bail!("first run requires an interactive terminal to confirm download");
    }

    print!("Press Enter to download, or Ctrl-C to abort: ");
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin()
        .read_line(&mut buf)
        .context("reading confirmation")?;
    if cancel::cancelled() {
        anyhow::bail!("aborted before download");
    }

    for (name, url, path, size) in assets {
        download_with_progress(name, url, &path, size, &client)?;
        if cancel::cancelled() {
            anyhow::bail!("download cancelled");
        }
    }
    Ok(())
}

struct TempFile {
    path: PathBuf,
    committed: bool,
}

impl TempFile {
    fn new(path: PathBuf) -> Self {
        Self { path, committed: false }
    }

    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn download_with_progress(
    name: &str,
    url: &str,
    path: &Path,
    expected: Option<u64>,
    client: &reqwest::blocking::Client,
) -> Result<()> {
    let tmp = TempFile::new(path.with_extension("download"));

    {
        let cleanup = tmp.path.clone();
        cancel::on_cancel(move || {
            let _ = std::fs::remove_file(&cleanup);
        });
    }

    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;
    let total = resp.content_length().or(expected);

    let pb = match total {
        Some(n) => {
            let pb = ProgressBar::new(n);
            pb.set_style(
                ProgressStyle::with_template(
                    "{msg} [{bar:30}] {bytes}/{total_bytes} {bytes_per_sec} eta {eta}",
                )
                .unwrap()
                .progress_chars("=>-"),
            );
            pb
        }
        None => {
            let pb = ProgressBar::new_spinner();
            pb.enable_steady_tick(Duration::from_millis(100));
            pb
        }
    };
    pb.set_message(name.to_string());

    let mut file = BufWriter::new(
        File::create(&tmp.path).with_context(|| format!("creating {}", tmp.path.display()))?,
    );
    let mut buf = [0u8; 64 * 1024];
    loop {
        if cancel::cancelled() {
            pb.abandon_with_message(format!("{name} cancelled"));
            anyhow::bail!("download cancelled");
        }
        let n = resp.read(&mut buf).context("reading from server")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("writing to disk")?;
        pb.inc(n as u64);
    }
    file.flush().context("flushing file")?;
    drop(file);
    std::fs::rename(&tmp.path, path)
        .with_context(|| format!("renaming {} to {}", tmp.path.display(), path.display()))?;
    pb.finish_with_message(format!("{name} done"));
    tmp.commit();
    Ok(())
}

fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = n as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

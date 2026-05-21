use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cancel;

const MODEL_URL: &str = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx";
const VOICES_URL: &str = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin";
const ESPEAK_DATA_URL: &str = concat!(
    "https://github.com/jonstuebe/yap/releases/download/v",
    env!("CARGO_PKG_VERSION"),
    "/espeak-ng-data.tar.gz"
);
const ESPEAK_DATA_DIR_NAME: &str = "espeak-ng-data";

pub fn base_dir() -> Result<PathBuf> {
    let base = dirs::data_dir()
        .context("could not locate user data dir")?
        .join("yap");
    std::fs::create_dir_all(&base).with_context(|| format!("creating {}", base.display()))?;
    Ok(base)
}

pub fn paths() -> Result<(PathBuf, PathBuf)> {
    let base = base_dir()?;
    Ok((base.join("kokoro-v1.0.onnx"), base.join("voices-v1.0.bin")))
}

pub fn espeak_data_parent() -> Result<PathBuf> {
    base_dir()
}

enum AssetKind {
    File(PathBuf),
    Tarball { extract_to: PathBuf },
}

struct Asset {
    name: &'static str,
    url: &'static str,
    kind: AssetKind,
}

pub fn ensure() -> Result<()> {
    let (model_path, voices_path) = paths()?;
    let espeak_parent = espeak_data_parent()?;

    let mut missing: Vec<Asset> = Vec::new();
    if !model_path.exists() {
        missing.push(Asset {
            name: "kokoro-v1.0.onnx",
            url: MODEL_URL,
            kind: AssetKind::File(model_path),
        });
    }
    if !voices_path.exists() {
        missing.push(Asset {
            name: "voices-v1.0.bin",
            url: VOICES_URL,
            kind: AssetKind::File(voices_path),
        });
    }
    let espeak_marker = espeak_parent.join(ESPEAK_DATA_DIR_NAME).join("phontab");
    if !espeak_marker.exists() {
        missing.push(Asset {
            name: "espeak-ng-data.tar.gz",
            url: ESPEAK_DATA_URL,
            kind: AssetKind::Tarball {
                extract_to: espeak_parent,
            },
        });
    }

    if missing.is_empty() {
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("building http client")?;

    let mut sized: Vec<(Asset, Option<u64>)> = Vec::with_capacity(missing.len());
    for asset in missing {
        let size = client
            .head(asset.url)
            .send()
            .with_context(|| format!("HEAD {}", asset.url))?
            .content_length();
        sized.push((asset, size));
    }

    let total_known: u64 = sized.iter().filter_map(|(_, s)| *s).sum();

    println!("yap needs to download a few things on first run:");
    for (asset, size) in &sized {
        let size_str = size.map(human_bytes).unwrap_or_else(|| "?".into());
        let dest = match &asset.kind {
            AssetKind::File(p) => p.display().to_string(),
            AssetKind::Tarball { extract_to, .. } => {
                format!("{}/ (extracted)", extract_to.display())
            }
        };
        println!("  • {} ({size_str}) → {dest}", asset.name);
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

    for (asset, size) in sized {
        match asset.kind {
            AssetKind::File(path) => {
                download_to_file(asset.name, asset.url, &path, size, &client)?;
            }
            AssetKind::Tarball { extract_to } => {
                download_and_extract_tarball(asset.name, asset.url, &extract_to, size, &client)?;
            }
        }
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

fn download_to_file(
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

    let file = File::create(&tmp.path)
        .with_context(|| format!("creating {}", tmp.path.display()))?;
    stream_download(name, url, expected, client, BufWriter::new(file))?;

    std::fs::rename(&tmp.path, path)
        .with_context(|| format!("renaming {} to {}", tmp.path.display(), path.display()))?;
    tmp.commit();
    Ok(())
}

fn download_and_extract_tarball(
    name: &str,
    url: &str,
    extract_to: &Path,
    expected: Option<u64>,
    client: &reqwest::blocking::Client,
) -> Result<()> {
    let tmp_path = extract_to.join(format!(".{name}.download"));
    let tmp = TempFile::new(tmp_path);
    {
        let cleanup = tmp.path.clone();
        cancel::on_cancel(move || {
            let _ = std::fs::remove_file(&cleanup);
        });
    }

    std::fs::create_dir_all(extract_to)
        .with_context(|| format!("creating {}", extract_to.display()))?;

    let file = File::create(&tmp.path)
        .with_context(|| format!("creating {}", tmp.path.display()))?;
    stream_download(name, url, expected, client, BufWriter::new(file))?;

    let reader = File::open(&tmp.path)
        .with_context(|| format!("opening {} for extract", tmp.path.display()))?;
    let gz = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(gz);
    archive
        .unpack(extract_to)
        .with_context(|| format!("extracting {} to {}", name, extract_to.display()))?;

    drop(tmp); // removes the tarball
    Ok(())
}

fn stream_download<W: Write>(
    name: &str,
    url: &str,
    expected: Option<u64>,
    client: &reqwest::blocking::Client,
    mut sink: W,
) -> Result<()> {
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
        sink.write_all(&buf[..n]).context("writing to disk")?;
        pb.inc(n as u64);
    }
    sink.flush().context("flushing")?;
    pb.finish_with_message(format!("{name} done"));
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

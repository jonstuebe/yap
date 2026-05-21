use anyhow::{Context, Result};
use std::process::Command;

const INSTALL_URL: &str = "https://raw.githubusercontent.com/jonstuebe/yap/main/install.sh";

pub fn run() -> Result<()> {
    let current_exe = std::env::current_exe().context("locating current binary")?;
    let install_dir = current_exe
        .parent()
        .context("current binary has no parent dir")?;

    println!(
        "Updating yap (current: v{}, install dir: {})",
        env!("CARGO_PKG_VERSION"),
        install_dir.display()
    );

    let cmd = format!("curl -fsSL {INSTALL_URL} | sh");
    let status = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .env("YAP_INSTALL_DIR", install_dir)
        .status()
        .context("running update")?;
    if !status.success() {
        anyhow::bail!("update failed");
    }
    Ok(())
}

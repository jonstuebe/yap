use anyhow::{Context, Result};
use std::process::Command;

pub fn read() -> Result<String> {
    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        Ok(text) => Ok(text),
        Err(_) => {
            let out = Command::new("pbpaste")
                .output()
                .context("pbpaste failed and arboard did not return text")?;
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        }
    }
}

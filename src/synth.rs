use anyhow::{Context, Result, anyhow};
use kokoros::tts::koko::TTSKoko;
use std::path::PathBuf;

pub const SAMPLE_RATE: u32 = 24_000;

pub struct Synth {
    tts: TTSKoko,
}

impl Synth {
    pub fn load() -> Result<Self> {
        let (model_path, voices_path) = model_paths()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("building tokio runtime")?;
        let model = model_path.to_string_lossy().into_owned();
        let voices = voices_path.to_string_lossy().into_owned();
        let tts = runtime.block_on(async move { TTSKoko::new(&model, &voices).await });
        Ok(Self { tts })
    }

    pub fn synthesize(&self, text: &str, voice: &str, speed: f32, lang: &str) -> Result<Vec<f32>> {
        self.tts
            .tts_raw_audio(text, lang, voice, speed, None, None, None, None)
            .map_err(|e| anyhow!("kokoro synthesis failed: {e}"))
    }

    pub fn voices(&self) -> Vec<String> {
        self.tts.get_available_voices()
    }
}

fn model_paths() -> Result<(PathBuf, PathBuf)> {
    let base = dirs::data_dir()
        .context("could not locate user data dir")?
        .join("yap");
    std::fs::create_dir_all(&base).with_context(|| format!("creating {}", base.display()))?;
    Ok((
        base.join("kokoro-v1.0.onnx"),
        base.join("voices-v1.0.bin"),
    ))
}

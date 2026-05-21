use anyhow::{Context, Result, anyhow};
use kokoros::tts::koko::TTSKoko;

use crate::assets;

pub const SAMPLE_RATE: u32 = 24_000;

pub struct Synth {
    tts: TTSKoko,
}

impl Synth {
    pub fn load() -> Result<Self> {
        let (model_path, voices_path) = assets::paths()?;
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

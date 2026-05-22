use anyhow::{Context, Result, anyhow, bail};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};

pub enum Sink {
    Wav(hound::WavWriter<BufWriter<File>>),
    Mp3(Mp3Writer),
}

pub struct Mp3Writer {
    encoder: mp3lame_encoder::Encoder,
    out: BufWriter<File>,
}

impl Sink {
    pub fn create(path: &Path, sample_rate: u32) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());
        match ext.as_deref() {
            Some("wav") => {
                let spec = hound::WavSpec {
                    channels: 1,
                    sample_rate,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                };
                let writer = hound::WavWriter::create(path, spec)
                    .with_context(|| format!("creating WAV file {}", path.display()))?;
                Ok(Sink::Wav(writer))
            }
            Some("mp3") => {
                let mut builder = Builder::new().ok_or_else(|| anyhow!("init LAME encoder"))?;
                builder
                    .set_num_channels(1)
                    .map_err(|e| anyhow!("LAME channels: {e}"))?;
                builder
                    .set_sample_rate(sample_rate)
                    .map_err(|e| anyhow!("LAME sample rate: {e}"))?;
                builder
                    .set_brate(Bitrate::Kbps128)
                    .map_err(|e| anyhow!("LAME bitrate: {e}"))?;
                builder
                    .set_quality(Quality::Best)
                    .map_err(|e| anyhow!("LAME quality: {e}"))?;
                let encoder = builder.build().map_err(|e| anyhow!("LAME build: {e}"))?;
                let file = File::create(path)
                    .with_context(|| format!("creating MP3 file {}", path.display()))?;
                Ok(Sink::Mp3(Mp3Writer {
                    encoder,
                    out: BufWriter::new(file),
                }))
            }
            Some(other) => bail!("unsupported output extension .{other} — use .wav or .mp3"),
            None => bail!("output path must have a .wav or .mp3 extension"),
        }
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        match self {
            Sink::Wav(writer) => {
                for &s in samples {
                    let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    writer.write_sample(v).context("writing WAV sample")?;
                }
                Ok(())
            }
            Sink::Mp3(w) => {
                let pcm: Vec<i16> = samples
                    .iter()
                    .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .collect();
                let mut buf: Vec<u8> =
                    Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(pcm.len()));
                let n = w
                    .encoder
                    .encode(MonoPcm(&pcm), buf.spare_capacity_mut())
                    .map_err(|e| anyhow!("MP3 encode: {e}"))?;
                // SAFETY: encoder reports `n` bytes initialized in the spare capacity.
                unsafe { buf.set_len(n) };
                w.out.write_all(&buf).context("writing MP3 bytes")?;
                Ok(())
            }
        }
    }

    pub fn finalize(self) -> Result<()> {
        match self {
            Sink::Wav(writer) => {
                writer.finalize().context("finalizing WAV file")?;
                Ok(())
            }
            Sink::Mp3(mut w) => {
                let mut tail: Vec<u8> =
                    Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(0));
                let n = w
                    .encoder
                    .flush::<FlushNoGap>(tail.spare_capacity_mut())
                    .map_err(|e| anyhow!("MP3 flush: {e}"))?;
                // SAFETY: encoder reports `n` bytes initialized in the spare capacity.
                unsafe { tail.set_len(n) };
                w.out.write_all(&tail).context("writing MP3 tail")?;
                w.out.flush().context("flushing MP3 file")?;
                Ok(())
            }
        }
    }
}


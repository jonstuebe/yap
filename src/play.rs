use anyhow::{Context, Result};
use rodio::{OutputStream, Sink, buffer::SamplesBuffer};
use std::sync::Arc;

pub struct Player {
    _stream: OutputStream,
    sink: Arc<Sink>,
}

impl Player {
    pub fn new() -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().context("opening default audio output stream")?;
        let sink = Sink::try_new(&handle).context("creating audio sink")?;
        Ok(Self {
            _stream: stream,
            sink: Arc::new(sink),
        })
    }

    pub fn play_blocking(&self, samples: Vec<f32>, sample_rate: u32) {
        self.sink.append(SamplesBuffer::new(1, sample_rate, samples));
        self.sink.sleep_until_end();
    }

    pub fn sink(&self) -> Arc<Sink> {
        self.sink.clone()
    }
}

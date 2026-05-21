# yap

`pbpaste`, but it talks. Reads your macOS clipboard aloud using [Kokoro TTS](https://github.com/thewh1teagle/kokoro-onnx).

Synthesis streams chunk-by-chunk, so audio starts playing as soon as the first sentence is ready instead of waiting for the whole clipboard.

## Install

### From source

Requires Rust (1.93+) and CMake. On macOS:

```sh
brew install cmake
cargo install --git https://github.com/jonstuebe/yap.git
```

That puts a `yap` binary on your `PATH`. On first run, yap downloads the Kokoro model files (~340MB) to `~/Library/Application Support/yap/`.

## Usage

Copy some text, then:

```sh
yap
```

Hit `Ctrl-C` to stop playback.

### Options

```sh
yap --voice af_heart --speed 1.1 --lang en-us
yap --list-voices
```

| Flag            | Default    | Description           |
| --------------- | ---------- | --------------------- |
| `--voice`       | `af_heart` | Voice name            |
| `--speed`       | `1.0`      | Speech rate           |
| `--lang`        | `en-us`    | Language code         |
| `--list-voices` | —          | Print voices and exit |

## How it works

1. Reads the clipboard via [`arboard`](https://github.com/1Password/arboard) (with a `pbpaste` fallback).
2. Splits text on sentence boundaries, hard-cutting any sentence over 400 chars to stay under Kokoro's ~510-token ceiling.
3. A producer thread synthesizes each chunk into raw f32 samples via [Kokoros](https://github.com/lucasjinreal/Kokoros) (ONNX Runtime + espeak-ng, both static-linked). The main thread plays them in order through [`rodio`](https://github.com/RustAudio/rodio). A bounded channel keeps the producer ~1–2 chunks ahead.

## Platform

Apple Silicon macOS only. The release binary is fully self-contained — no Python, no separate ONNX Runtime, no espeak-ng install. `otool -L` shows only Apple system frameworks.

## License

GPL-3.0-or-later (transitive from espeak-ng).

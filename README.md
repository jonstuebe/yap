# yap

`pbpaste`, but it talks. Reads your macOS clipboard aloud using [Kokoro TTS](https://github.com/thewh1teagle/kokoro-onnx).

Synthesis streams chunk-by-chunk, so audio starts playing as soon as the first sentence is ready instead of waiting for the whole clipboard.

## Install

Apple Silicon macOS only.

```sh
curl -fsSL https://raw.githubusercontent.com/jonstuebe/yap/main/install.sh | sh
```

Installs to `/usr/local/bin/yap` (falls back to `~/.local/bin` if not writable). Override the install location with `YAP_INSTALL_DIR=...`.

On first run, yap downloads the Kokoro model files (~340MB) to `~/Library/Application Support/yap/`.

> The release binary is unsigned. The installer downloads via `curl`, which doesn't set the Gatekeeper quarantine bit, so the binary runs without prompts. If you instead download the tarball from the GitHub Releases page in a browser, run `xattr -c /usr/local/bin/yap` after installing to clear the quarantine attribute.

### From source

Requires Rust (1.93+) and CMake:

```sh
brew install cmake
cargo install --git https://github.com/jonstuebe/yap.git
```

## Usage

Copy some text, then:

```sh
yap
```

Hit `Ctrl-C` to stop playback.

### Watch mode

```sh
yap --watch
```

Monitors the clipboard and speaks whatever you copy. Copying again while it's
talking cancels the current playback and starts on the new selection. `Ctrl-C`
quits.

### Options

```sh
yap --voice af_heart --speed 1.1 --lang en-us
yap --list-voices
```

| Flag            | Default    | Description                            |
| --------------- | ---------- | -------------------------------------- |
| `--voice`       | `af_heart` | Voice name                             |
| `--speed`       | `1.0`      | Speech rate                            |
| `--lang`        | `en-us`    | Language code                          |
| `--watch`       | —          | Speak on every clipboard change        |
| `--list-voices` | —          | Print voices and exit                  |

### Update

```sh
yap update
```

Re-runs the installer to fetch the latest release, replacing the binary in the same directory it's currently installed in.

## How it works

1. Reads the clipboard via [`arboard`](https://github.com/1Password/arboard) (with a `pbpaste` fallback).
2. Splits text on sentence boundaries, hard-cutting any sentence over 400 chars to stay under Kokoro's ~510-token ceiling.
3. A producer thread synthesizes each chunk into raw f32 samples via [Kokoros](https://github.com/lucasjinreal/Kokoros) (ONNX Runtime + espeak-ng, both static-linked). The main thread plays them in order through [`rodio`](https://github.com/RustAudio/rodio). A bounded channel keeps the producer ~1–2 chunks ahead.

## Platform

Apple Silicon macOS only. The release binary is fully self-contained — no Python, no separate ONNX Runtime, no espeak-ng install. `otool -L` shows only Apple system frameworks.

## License

GPL-3.0-or-later (transitive from espeak-ng).

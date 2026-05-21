# tts

`pbpaste`, but it talks. Reads your macOS clipboard aloud using [Kokoro TTS](https://github.com/thewh1teagle/kokoro-onnx).

Synthesis streams chunk-by-chunk, so audio starts playing as soon as the first sentence is ready instead of waiting for the whole clipboard.

## Requirements

- macOS (uses `pbpaste` and `afplay`)
- Python 3.12+
- [uv](https://github.com/astral-sh/uv)

## Setup

```sh
uv sync
```

Download the Kokoro model files into `models/`:

```sh
mkdir -p models
curl -L -o models/kokoro-v1.0.onnx https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx
curl -L -o models/voices-v1.0.bin   https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin
```

## Usage

Copy some text, then:

```sh
uv run main.py
```

Hit `Ctrl-C` to stop playback.

### Options

```sh
uv run main.py --voice af_heart --speed 1.1 --lang en-us
uv run main.py --list-voices
```

| Flag            | Default    | Description              |
| --------------- | ---------- | ------------------------ |
| `--voice`       | `af_heart` | Voice name               |
| `--speed`       | `1.0`      | Speech rate              |
| `--lang`        | `en-us`    | Language code            |
| `--list-voices` | —          | Print voices and exit    |

## How it works

1. Reads the clipboard with `pbpaste`.
2. Splits the text on sentence boundaries (hard-cut at 400 chars to stay under Kokoro's ~510-token ceiling).
3. A producer thread synthesizes each chunk into a temp WAV; the main thread plays them in order with `afplay`. A small bounded queue keeps the producer ~1–2 chunks ahead of the player.

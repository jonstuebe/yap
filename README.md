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

### Options

```sh
yap --voice af_heart --speed 1.1 --lang en-us
yap --list-voices
yap --save out.wav   # or out.mp3
```

| Flag            | Default    | Description                                       |
| --------------- | ---------- | ------------------------------------------------- |
| `--voice`       | `af_heart` | Voice name                                        |
| `--speed`       | `1.0`      | Speech rate                                       |
| `--lang`        | `en-us`    | Language code                                     |
| `--list-voices` | —          | Print voices and exit                             |
| `--save PATH`   | —          | Write audio to `.wav` or `.mp3` instead of playing |

### Update

```sh
yap update
```

Re-runs the installer to fetch the latest release, replacing the binary in the same directory it's currently installed in.

## Global hotkey

`yap` is a one-shot CLI, so binding a system-wide hotkey is best handled outside the binary.

### macOS Shortcuts

1. Open **Shortcuts.app** and create a new shortcut named *Yap Clipboard*.
2. Add the **Run Shell Script** action with:
   ```sh
   /usr/local/bin/yap
   ```
   (Or `~/.local/bin/yap` if you installed there. Use the full path — Shortcuts doesn't inherit your shell's `PATH`.)
3. In the shortcut's info panel (⌘I), set a **Keyboard Shortcut** — e.g. `⌃⌥⌘Y`.

Running the shortcut a second time won't stop playback — it spawns another instance. To stop, either focus the terminal it launched from and hit `Ctrl-C`, or wire up a second shortcut that runs `killall yap`.

### Raycast

A ready-to-use script command lives at [`extras/raycast/yap.sh`](extras/raycast/yap.sh). It auto-detects whether `yap` is installed in `/usr/local/bin` or `~/.local/bin`.

1. In Raycast settings → **Extensions** → **Script Commands**, note your **Script Directories** path (add one if you don't have it set — e.g. `~/.raycast-scripts`).
2. Drop the script into that directory:
   ```sh
   mkdir -p ~/.raycast-scripts
   curl -fsSL https://raw.githubusercontent.com/jonstuebe/yap/main/extras/raycast/yap.sh \
     -o ~/.raycast-scripts/yap.sh
   chmod +x ~/.raycast-scripts/yap.sh
   ```
3. Back in Raycast settings → **Extensions**, find *Yap Clipboard* and assign a **Hotkey**.

Edit the `@raycast.mode` line in `yap.sh` from `silent` to `compact` if you want a HUD while it's speaking.

## How it works

1. Reads the clipboard via [`arboard`](https://github.com/1Password/arboard) (with a `pbpaste` fallback).
2. Splits text on sentence boundaries, hard-cutting any sentence over 400 chars to stay under Kokoro's ~510-token ceiling.
3. A producer thread synthesizes each chunk into raw f32 samples via [Kokoros](https://github.com/lucasjinreal/Kokoros) (ONNX Runtime + espeak-ng, both static-linked). The main thread plays them in order through [`rodio`](https://github.com/RustAudio/rodio). A bounded channel keeps the producer ~1–2 chunks ahead.

## Platform

Apple Silicon macOS only. The release binary is fully self-contained — no Python, no separate ONNX Runtime, no espeak-ng install. `otool -L` shows only Apple system frameworks.

## License

GPL-3.0-or-later (transitive from espeak-ng).

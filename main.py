#!/usr/bin/env python3
"""tts-clip: read the macOS clipboard and speak it with Kokoro."""

import argparse
import itertools
import os
import queue
import re
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent
MODEL_PATH = PROJECT_ROOT / "models" / "kokoro-v1.0.onnx"
VOICES_PATH = PROJECT_ROOT / "models" / "voices-v1.0.bin"
DEFAULT_VOICE = "af_heart"

# Kokoro's hard ceiling is ~510 phoneme tokens. 400 chars of English stays
# comfortably under that even with dense words.
MAX_CHUNK_CHARS = 400


def read_clipboard() -> str:
    result = subprocess.run(["pbpaste"], capture_output=True, text=True, check=True)
    return result.stdout


def chunk_text(text: str) -> list[str]:
    """Split on sentence boundaries; hard-cut any sentence that's still too long."""
    text = text.strip()
    if not text:
        return []

    sentences = re.split(r"(?<=[.!?])\s+", text)
    chunks: list[str] = []
    for sentence in sentences:
        sentence = sentence.strip()
        if not sentence:
            continue
        if len(sentence) <= MAX_CHUNK_CHARS:
            chunks.append(sentence)
        else:
            for i in range(0, len(sentence), MAX_CHUNK_CHARS):
                chunks.append(sentence[i : i + MAX_CHUNK_CHARS])
    return chunks


class Spinner:
    """Minimal stderr spinner. Quiet when stderr isn't a TTY."""

    FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]

    def __init__(self, label: str):
        self.label = label
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._enabled = sys.stderr.isatty()
        self._start_time = 0.0

    def __enter__(self):
        if not self._enabled:
            return self
        self._start_time = time.monotonic()
        self._thread = threading.Thread(target=self._spin, daemon=True)
        self._thread.start()
        return self

    def _spin(self):
        for frame in itertools.cycle(self.FRAMES):
            if self._stop.is_set():
                break
            elapsed = time.monotonic() - self._start_time
            sys.stderr.write(f"\r{frame} {self.label} {elapsed:4.1f}s")
            sys.stderr.flush()
            time.sleep(0.08)

    def __exit__(self, exc_type, exc, tb):
        if not self._enabled:
            return
        self._stop.set()
        if self._thread:
            self._thread.join()
        elapsed = time.monotonic() - self._start_time
        if exc_type is KeyboardInterrupt:
            status = "⏹ cancelled"
        elif exc_type:
            status = "✗"
        else:
            status = "✓"
        sys.stderr.write(f"\r\033[K{status} {self.label} {elapsed:.1f}s\n")
        sys.stderr.flush()


# Sentinel value pushed onto the queue to signal the producer is done.
_END = object()


def stream_play(chunks: list[str], voice: str, speed: float, lang: str) -> None:
    """Producer synthesizes chunks; consumer (this thread) plays them via afplay."""

    from kokoro_onnx import Kokoro
    import soundfile as sf

    kokoro = Kokoro(str(MODEL_PATH), str(VOICES_PATH))
    total = len(chunks)

    # Small bounded queue: stay 1–2 chunks ahead so we don't waste synthesis if
    # the user cancels, and so memory stays small.
    audio_queue: queue.Queue = queue.Queue(maxsize=2)
    cancel_event = threading.Event()
    temp_files: list[str] = []
    producer_error: list[BaseException] = []

    def producer() -> None:
        try:
            for i, chunk in enumerate(chunks):
                if cancel_event.is_set():
                    break
                samples, sample_rate = kokoro.create(chunk, voice=voice, speed=speed, lang=lang)
                if cancel_event.is_set():
                    break
                with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as fp:
                    sf.write(fp.name, samples, sample_rate)
                    path = fp.name
                temp_files.append(path)
                # put() blocks when the queue is full — natural backpressure.
                while not cancel_event.is_set():
                    try:
                        audio_queue.put((i, path), timeout=0.2)
                        break
                    except queue.Full:
                        continue
        except BaseException as exc:
            producer_error.append(exc)
        finally:
            audio_queue.put(_END)

    producer_thread = threading.Thread(target=producer, daemon=True)
    producer_thread.start()

    current_proc: subprocess.Popen | None = None
    tty = sys.stderr.isatty()

    def status_line(msg: str) -> None:
        if tty:
            sys.stderr.write(f"\r\033[K{msg}")
            sys.stderr.flush()

    try:
        # Wait for the first chunk with a spinner — this is the latency the user feels.
        with Spinner(f"synthesizing first of {total} chunk(s)"):
            first = audio_queue.get()

        if first is _END:
            if producer_error:
                raise producer_error[0]
            return

        item = first
        while item is not _END:
            i, path = item
            status_line(f"▶ playing {i + 1}/{total}")
            current_proc = subprocess.Popen(["afplay", path])
            current_proc.wait()
            current_proc = None
            try:
                os.unlink(path)
            except OSError:
                pass
            item = audio_queue.get()

        if tty:
            sys.stderr.write("\r\033[K")
            sys.stderr.flush()

        if producer_error:
            raise producer_error[0]
    except KeyboardInterrupt:
        cancel_event.set()
        if current_proc and current_proc.poll() is None:
            current_proc.terminate()
            try:
                current_proc.wait(timeout=1)
            except subprocess.TimeoutExpired:
                current_proc.kill()
        raise
    finally:
        cancel_event.set()
        # Drain anything the producer queued after we stopped consuming.
        while True:
            try:
                leftover = audio_queue.get_nowait()
            except queue.Empty:
                break
            if leftover is _END or not isinstance(leftover, tuple):
                continue
            temp_files.append(leftover[1])
        for path in temp_files:
            try:
                os.unlink(path)
            except OSError:
                pass


def main() -> int:
    parser = argparse.ArgumentParser(description="Speak clipboard text with Kokoro TTS.")
    parser.add_argument("--voice", default=DEFAULT_VOICE, help=f"voice name (default: {DEFAULT_VOICE})")
    parser.add_argument("--speed", type=float, default=1.0, help="speech rate (default: 1.0)")
    parser.add_argument("--lang", default="en-us", help="language code (default: en-us)")
    parser.add_argument("--list-voices", action="store_true", help="list available voices and exit")
    args = parser.parse_args()

    # Silence onnxruntime / phonemizer noise on stderr unless something fails.
    os.environ.setdefault("ORT_LOGGING_LEVEL", "3")

    if args.list_voices:
        from kokoro_onnx import Kokoro

        kokoro = Kokoro(str(MODEL_PATH), str(VOICES_PATH))
        for v in sorted(kokoro.get_voices()):
            print(v)
        return 0

    text = read_clipboard().strip()
    if not text:
        print("clipboard is empty", file=sys.stderr)
        return 1

    chunks = chunk_text(text)
    if not chunks:
        print("clipboard is empty", file=sys.stderr)
        return 1

    stream_play(chunks, voice=args.voice, speed=args.speed, lang=args.lang)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\ncancelled", file=sys.stderr)
        sys.exit(130)

#!/bin/bash

# @raycast.schemaVersion 1
# @raycast.title Yap Clipboard
# @raycast.mode silent
# @raycast.packageName yap
# @raycast.icon 🗣️
# @raycast.description Speak clipboard contents aloud using Kokoro TTS.

if [ -x /usr/local/bin/yap ]; then
  exec /usr/local/bin/yap
elif [ -x "$HOME/.local/bin/yap" ]; then
  exec "$HOME/.local/bin/yap"
else
  echo "yap not found in /usr/local/bin or ~/.local/bin" >&2
  exit 1
fi

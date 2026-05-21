#!/bin/sh
# Install yap. Usage: curl -fsSL https://raw.githubusercontent.com/jonstuebe/yap/main/install.sh | sh
set -e

REPO="jonstuebe/yap"
BINARY="yap"
ARCHIVE="yap-aarch64-apple-darwin.tar.gz"

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
  echo "Error: yap requires Apple Silicon macOS (arm64)." >&2
  exit 1
fi

INSTALL_DIR_EXPLICIT="${YAP_INSTALL_DIR:-}"
INSTALL_DIR="${YAP_INSTALL_DIR:-/usr/local/bin}"
if ! mkdir -p "$INSTALL_DIR" 2>/dev/null || [ ! -w "$INSTALL_DIR" ]; then
  if [ -z "$INSTALL_DIR_EXPLICIT" ] && mkdir -p "$HOME/.local/bin" 2>/dev/null && [ -w "$HOME/.local/bin" ]; then
    INSTALL_DIR="$HOME/.local/bin"
    echo "Installing to $INSTALL_DIR (no write access to /usr/local/bin)."
  else
    echo "Error: cannot write to $INSTALL_DIR. Re-run with sudo or set YAP_INSTALL_DIR." >&2
    exit 1
  fi
fi

TAG="${YAP_VERSION:-}"
if [ -z "$TAG" ]; then
  TAG=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
fi
if [ -z "$TAG" ]; then
  echo "Error: could not resolve latest release tag." >&2
  exit 1
fi

EXISTING_BIN="$INSTALL_DIR/$BINARY"
if [ -x "$EXISTING_BIN" ]; then
  EXISTING_VERSION=$("$EXISTING_BIN" --version 2>/dev/null | awk 'NR==1 {print $NF}')
  if [ -n "$EXISTING_VERSION" ] && [ "v$EXISTING_VERSION" = "$TAG" ]; then
    echo "yap $TAG is already installed at $EXISTING_BIN."
    exit 0
  fi
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

URL="https://github.com/$REPO/releases/download/$TAG/$ARCHIVE"
echo "Downloading yap $TAG..."
curl -fsSL "$URL" -o "$TMP/$ARCHIVE"
curl -fsSL "$URL.sha256" -o "$TMP/$ARCHIVE.sha256"

EXPECTED=$(awk '{print $1}' "$TMP/$ARCHIVE.sha256")
ACTUAL=$(shasum -a 256 "$TMP/$ARCHIVE" | awk '{print $1}')
if [ "$EXPECTED" != "$ACTUAL" ]; then
  echo "Error: checksum mismatch (expected $EXPECTED, got $ACTUAL)." >&2
  exit 1
fi

tar -xzf "$TMP/$ARCHIVE" -C "$TMP"
chmod +x "$TMP/$BINARY"
mv "$TMP/$BINARY" "$INSTALL_DIR/$BINARY"

echo "Installed $BINARY $TAG to $INSTALL_DIR/$BINARY"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    SHELL_NAME=$(basename "${SHELL:-}")
    case "$SHELL_NAME" in
      zsh)  PROFILE="~/.zshrc" ;;
      bash) PROFILE="~/.bashrc (or ~/.bash_profile on macOS)" ;;
      fish) PROFILE="~/.config/fish/config.fish" ;;
      *)    PROFILE="your shell profile" ;;
    esac
    echo
    echo "Note: $INSTALL_DIR is not in your PATH."
    if [ "$SHELL_NAME" = "fish" ]; then
      echo "Add this line to $PROFILE:"
      echo "  set -gx PATH $INSTALL_DIR \$PATH"
    else
      echo "Add this line to $PROFILE:"
      echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    fi
    echo "Then restart your shell or run: source $PROFILE"
    ;;
esac

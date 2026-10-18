#!/bin/sh
# LAST MD installer for Arch/Omarchy (works on most Linux).
#
#   curl -fsSL https://raw.githubusercontent.com/leoplanet/last-md/master/install.sh | sh
#
# Downloads the prebuilt binary from the latest release and installs it to
# ~/.local/bin (no sudo). Falls back to building from source if the download
# fails or your arch has no prebuilt.
set -eu

REPO="leoplanet/last-md"
TAG="v0.1.0-alpha"
BIN="last-md"

case "$(uname -m)" in
  x86_64)  asset="$BIN-x86_64-linux" ;;
  *)       asset="" ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

install_to() {
  # $1 = binary file
  mkdir -p "$HOME/.local/bin"
  install -m755 "$1" "$HOME/.local/bin/$BIN"
}

if [ -n "$asset" ]; then
  echo "Downloading $asset from $TAG..."
  if curl -fsSL "https://github.com/$REPO/releases/download/$TAG/$asset" -o "$tmp/$BIN"; then
    install_to "$tmp/$BIN"
    echo "Installed $BIN to ~/.local/bin/$BIN"
    echo "Make sure ~/.local/bin is on your PATH, then run: $BIN [file.md]"
    exit 0
  fi
  echo "Download failed — building from source instead..."
fi

command -v cargo >/dev/null 2>&1 || {
  echo "cargo not found. Install the Rust toolchain first: https://rustup.rs"
  exit 1
}
git clone --depth 1 "https://github.com/$REPO" "$tmp/src"
cd "$tmp/src"
cargo build --release
install_to target/release/last_md
echo "Installed $BIN (built from source) to ~/.local/bin/$BIN"

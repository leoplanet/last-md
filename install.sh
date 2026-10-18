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
TAG="v0.1.1-alpha"
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

# App launcher entry + icon. $1 = directory holding the repo files
# (a source checkout), or empty to fetch them from GitHub.
install_launcher() {
  apps="$HOME/.local/share/applications"
  icons="$HOME/.local/share/icons/hicolor/256x256/apps"
  mkdir -p "$apps" "$icons"
  if [ -n "${1:-}" ]; then
    cp "$1/aur/last-md-git/last-md.desktop" "$tmp/$BIN.desktop"
    cp "$1/assets/icon.png" "$tmp/$BIN.png"
  else
    raw="https://raw.githubusercontent.com/$REPO/master"
    curl -fsSL "$raw/aur/last-md-git/last-md.desktop" -o "$tmp/$BIN.desktop" &&
      curl -fsSL "$raw/assets/icon.png" -o "$tmp/$BIN.png" || {
      echo "(Could not fetch the launcher entry; $BIN still works from a terminal.)"
      return 0
    }
  fi
  # Point the launcher at the installed binary, so it works without PATH.
  sed "s|^Exec=$BIN|Exec=$HOME/.local/bin/$BIN|" "$tmp/$BIN.desktop" > "$apps/$BIN.desktop"
  install -m644 "$tmp/$BIN.png" "$icons/$BIN.png"
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$apps" || true
  echo "Added $BIN to the app launcher"
}

if [ -n "$asset" ]; then
  echo "Downloading $asset from $TAG..."
  if curl -fsSL "https://github.com/$REPO/releases/download/$TAG/$asset" -o "$tmp/$BIN"; then
    install_to "$tmp/$BIN"
    install_launcher ""
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
install_launcher "$tmp/src"
echo "Installed $BIN (built from source) to ~/.local/bin/$BIN"

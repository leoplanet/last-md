#!/bin/sh
# Uninstall LAST MD (removes exactly what install.sh installed).
#
#   curl -fsSL https://raw.githubusercontent.com/leoplanet/last-md/master/uninstall.sh | sh
set -eu

BIN="$HOME/.local/bin/last-md"
found=""
for f in "$BIN" \
         "$HOME/.local/share/applications/last-md.desktop" \
         "$HOME/.local/share/icons/hicolor/256x256/apps/last-md.png"; do
  if [ -e "$f" ]; then
    rm "$f"
    echo "Removed $f"
    found=1
  fi
done
if [ -z "$found" ]; then
  echo "$BIN not found — nothing to uninstall."
  echo "(If you installed via AUR, use: yay -R last-md-git)"
fi

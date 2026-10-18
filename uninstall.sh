#!/bin/sh
# Uninstall LAST MD (removes exactly what install.sh installed).
#
#   curl -fsSL https://raw.githubusercontent.com/leoplanet/last-md/master/uninstall.sh | sh
set -eu

BIN="$HOME/.local/bin/last-md"
if [ -e "$BIN" ]; then
  rm "$BIN"
  echo "Removed $BIN"
else
  echo "$BIN not found — nothing to uninstall."
fi

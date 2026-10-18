#!/bin/sh
# Build (debug) and run Last-MD.
cd "$(dirname "$0")" && CARGO_TARGET_DIR=~/.cache/last_md_target cargo run

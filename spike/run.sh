#!/usr/bin/env bash
# Spike launcher. Builds if needed, then runs.
cd "$(dirname "$0")/src-tauri" || exit 1
cargo run --quiet "$@"

#!/usr/bin/env bash
# Release run. custom-protocol is required or Tauri serves devUrl and the
# window fails with "connection refused".
set -e
cd "$(dirname "$0")"
npm run build --silent
cd src-tauri
cargo build --release --bin soundbox-app --features custom-protocol
exec ./target/release/soundbox-app "$@"

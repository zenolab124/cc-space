#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "dev" ]]; then
  exec pnpm exec tauri "$@" --config src-tauri/tauri.dev.conf.json
fi

exec pnpm exec tauri "$@"

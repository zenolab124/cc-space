#!/bin/bash
# 把 routine runner 包装为独立 Helper App，给 LaunchServices/TCC 一个稳定 bundle 身份。

set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP="$DIR/src-tauri/target/release/bundle/macos/Monet.app"

if [ ! -d "$APP" ]; then
  echo "error: Monet.app not found at $APP" >&2
  exit 1
fi

RUNNER_BIN="$APP/Contents/MacOS/monet-routine-runner"
if [ ! -f "$RUNNER_BIN" ]; then
  echo "error: monet-routine-runner binary not found" >&2
  exit 1
fi

HELPER="$APP/Contents/Helpers/MonetRoutineRunner.app"
rm -rf "$HELPER"
mkdir -p "$HELPER/Contents/MacOS"
mv "$RUNNER_BIN" "$HELPER/Contents/MacOS/monet-routine-runner"

VERSION=$(plutil -extract CFBundleShortVersionString raw "$APP/Contents/Info.plist" 2>/dev/null || echo "0.0.0")
cp "$DIR/src-runner/Info.plist" "$HELPER/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$VERSION" "$HELPER/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$VERSION" "$HELPER/Contents/Info.plist"

echo "✓ MonetRoutineRunner.app v$VERSION bundled at $HELPER"

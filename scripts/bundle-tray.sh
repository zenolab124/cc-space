#!/bin/bash
# 把 monet-tray 二进制包装为独立 Helper App，放到主应用的 LoginItems 下。
# macOS 把 Contents/Library/LoginItems/ 下的 .app 视为完全独立进程。

set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP="$DIR/src-tauri/target/release/bundle/macos/Monet.app"

if [ ! -d "$APP" ]; then
  echo "error: Monet.app not found at $APP" >&2
  exit 1
fi

TRAY_BIN="$APP/Contents/MacOS/monet-tray"
if [ ! -f "$TRAY_BIN" ]; then
  echo "error: monet-tray binary not found" >&2
  exit 1
fi

# 重建 Helper App bundle（清掉旧签名/旧结构残留）
HELPER="$APP/Contents/Library/LoginItems/MonetTray.app"
rm -rf "$HELPER"
mkdir -p "$HELPER/Contents/MacOS"

# 移动二进制（不留在主 MacOS 目录里，避免 bundle 归属混淆）
mv "$TRAY_BIN" "$HELPER/Contents/MacOS/monet-tray"

# Info.plist：版本号跟随主应用
VERSION=$(plutil -extract CFBundleShortVersionString raw "$APP/Contents/Info.plist" 2>/dev/null || echo "0.0.0")
cp "$DIR/src-tray/Info.plist" "$HELPER/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$VERSION" "$HELPER/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$VERSION" "$HELPER/Contents/Info.plist"

# dev 工具不出厂
rm -f "$APP/Contents/MacOS/schema-probe"

echo "✓ MonetTray.app v$VERSION bundled at $HELPER"

bash "$DIR/scripts/bundle-runner.sh"

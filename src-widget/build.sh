#!/bin/bash
# 构建 Widget Extension + widget-updater，嵌入 Tauri .app bundle，签名，打 DMG
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# --- 参数 ---
SIGN_ID="${SIGN_ID:-Monet Signing}"
SIGNING_KEYCHAIN="$HOME/Library/Keychains/monet-signing.keychain-db"
SIGNING_PASS_FILE="$HOME/.monet/signing/keychain-password"
CONFIG="${1:-Release}"
_RAW_BUNDLE="${2:-../src-tauri/target/release/bundle/macos/Monet.app}"
APP_BUNDLE="$(cd "$(dirname "$_RAW_BUNDLE")" && pwd)/$(basename "$_RAW_BUNDLE")"
XCODE="${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}"

if [ ! -d "$APP_BUNDLE" ]; then
    echo "Error: App bundle not found: $APP_BUNDLE"
    echo "Run 'pnpm tauri build' first."
    exit 1
fi

if [ -f "$SIGNING_PASS_FILE" ] && [ -f "$SIGNING_KEYCHAIN" ]; then
    security unlock-keychain -p "$(cat "$SIGNING_PASS_FILE")" "$SIGNING_KEYCHAIN"
fi
IDENTITIES=$(security find-identity -v -p codesigning)
if ! grep -F "\"$SIGN_ID\"" <<< "$IDENTITIES" >/dev/null; then
    echo "Error: signing identity '$SIGN_ID' not found" >&2
    exit 1
fi
CODESIGN_ARGS=(--force --options runtime --sign "$SIGN_ID")

# --- 构建 Widget Extension ---
echo "=> Building widget extension..."
DEVELOPER_DIR="$XCODE" xcodegen generate --quiet 2>/dev/null || DEVELOPER_DIR="$XCODE" xcodegen generate
DEVELOPER_DIR="$XCODE" xcodebuild build \
    -project MonetWidget.xcodeproj \
    -target MonetWidgetExtension \
    -configuration "$CONFIG" \
    CODE_SIGNING_ALLOWED=NO \
    CONFIGURATION_BUILD_DIR=build/"$CONFIG" \
    -quiet

# --- 构建 widget-updater ---
echo "=> Building widget-updater..."
(cd ../src-tauri && cargo build --release --bin widget-updater 2>&1 | tail -1)

# --- 嵌入 ---
echo "=> Embedding into app bundle..."
PLUGINS_DIR="$APP_BUNDLE/Contents/PlugIns"
mkdir -p "$PLUGINS_DIR"
rm -rf "$PLUGINS_DIR/MonetWidgetExtension.appex"
cp -R "build/$CONFIG/MonetWidgetExtension.appex" "$PLUGINS_DIR/"
cp ../src-tauri/target/release/widget-updater "$APP_BUNDLE/Contents/MacOS/widget-updater"

# --- 签名 ---
echo "=> Signing..."

codesign "${CODESIGN_ARGS[@]}" \
    --entitlements MonetWidgetExtension.entitlements \
    "$PLUGINS_DIR/MonetWidgetExtension.appex"
for BIN in "$APP_BUNDLE/Contents/MacOS/"*; do
    NAME=$(basename "$BIN")
    [ "$NAME" = "app" ] && continue
    codesign "${CODESIGN_ARGS[@]}" \
        --identifier "io.github.zenolab124.monet.$NAME" "$BIN"
done
# Helper App（独立 menubar 进程）：嵌套 bundle 必须先签内层再签外层
TRAY_APP="$APP_BUNDLE/Contents/Library/LoginItems/MonetTray.app"
if [ -d "$TRAY_APP" ]; then
    codesign "${CODESIGN_ARGS[@]}" \
        --identifier "io.github.zenolab124.monet.tray" "$TRAY_APP"
fi
codesign "${CODESIGN_ARGS[@]}" \
    --entitlements ../src-tauri/Monet.entitlements "$APP_BUNDLE"
codesign --verify --deep --strict "$APP_BUNDLE"
SIGNATURE_DETAILS=$(codesign -dvv "$APP_BUNDLE" 2>&1)
grep -F "Authority=$SIGN_ID" <<< "$SIGNATURE_DETAILS" >/dev/null
if grep -F 'Signature=adhoc' <<< "$SIGNATURE_DETAILS" >/dev/null; then
    echo "Error: unexpected ad-hoc signature" >&2
    exit 1
fi

# --- 打 DMG ---
APP_NAME=$(basename "$APP_BUNDLE" .app)
VERSION=$(plutil -extract CFBundleShortVersionString raw "$APP_BUNDLE/Contents/Info.plist" 2>/dev/null || echo "0.0.0")
DMG_DIR=$(dirname "$APP_BUNDLE")/../dmg
DMG_PATH="$DMG_DIR/${APP_NAME}_${VERSION}_aarch64.dmg"
mkdir -p "$DMG_DIR"
rm -f "$DMG_PATH"

echo "=> Creating DMG..."
DMG_STAGE=$(mktemp -d)
cp -R "$APP_BUNDLE" "$DMG_STAGE/"
ln -s /Applications "$DMG_STAGE/Applications"
if diskutil image create from --help &>/dev/null; then
    diskutil image create from --format UDZO --volumeName "$APP_NAME" "$DMG_STAGE" "$DMG_PATH"
else
    hdiutil create -volname "$APP_NAME" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG_PATH" -quiet
fi
rm -rf "$DMG_STAGE"

# --- Updater 产物（.app.tar.gz + minisign 签名 + latest.json） ---
# 仅在提供 TAURI_SIGNING_PRIVATE_KEY 时生成（发版链路:CI 经 secrets 注入;
# 日常本地打包无密钥自动跳过,不阻塞）。私钥对应 tauri.conf plugins.updater.pubkey
UPDATER_DIR="$(dirname "$APP_BUNDLE")/../updater"
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    echo "=> Creating updater artifacts..."
    mkdir -p "$UPDATER_DIR"
    TARBALL="$UPDATER_DIR/${APP_NAME}_${VERSION}_aarch64.app.tar.gz"
    rm -f "$TARBALL" "$TARBALL.sig"
    tar czf "$TARBALL" -C "$(dirname "$APP_BUNDLE")" "$(basename "$APP_BUNDLE")"
    (cd .. && pnpm tauri signer sign "$TARBALL")
    node "$SCRIPT_DIR/../scripts/create-latest-json.mjs" "$VERSION" "$TARBALL" "$UPDATER_DIR/latest.json"
    echo "   Updater: $TARBALL (+.sig, latest.json)"
else
    echo "=> Skipping updater artifacts (TAURI_SIGNING_PRIVATE_KEY not set)"
fi

echo "=> Done!"
echo "   App: $APP_BUNDLE"
echo "   DMG: $DMG_PATH"

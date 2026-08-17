#!/bin/bash
# 发版：校验发布说明 → 升版本号 → 打 tag。分发产物由 Release CI 远端构建（含公证）。
# 用法：
#   pnpm release                 → 全局下一 patch（同时计入现有 Nightly）
#   pnpm release -- minor        → 基于全局最大版本升 minor
#   pnpm release -- major        → 基于全局最大版本升 major
#   pnpm release -- --local-build       → 额外在本地构建自测产物（自签，非分发用）
#   pnpm release -- minor --local-build → 两者可组合，顺序不限

set -euo pipefail

BUMP=patch
LOCAL_BUILD=0
for ARG in "$@"; do
  case "$ARG" in
    patch|minor|major) BUMP="$ARG" ;;
    --local-build) LOCAL_BUILD=1 ;;
    *)
      echo "✗ 未知参数 '$ARG'：版本类型只支持 patch/minor/major，本地自测构建用 --local-build。" >&2
      exit 1
      ;;
  esac
done

SIGN_ID=${SIGN_ID:-Monet Signing}
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)

# 工作区必须干净：不代替用户决定提交内容（并行开发时盲提交会混入无关改动）
if ! git diff --quiet || ! git diff --cached --quiet || [ -n "$(git ls-files --others --exclude-standard)" ]; then
  echo "✗ 工作区有未提交改动，请先自行提交（按主题切分）后再发版：" >&2
  git status --short >&2
  exit 1
fi

# 签名身份只在本地自测构建时才需要；默认路径不打包，不做此检查
if [[ "$LOCAL_BUILD" -eq 1 ]]; then
  IDENTITIES=$(security find-identity -v -p codesigning)
  if ! grep -F "\"$SIGN_ID\"" <<< "$IDENTITIES" >/dev/null; then
    echo "✗ 找不到代码签名身份 '$SIGN_ID'，已停止发版。" >&2
    exit 1
  fi
fi

# 正式版与 Nightly 共用一条版本序列。Git tag、package.json 和滚动 Nightly 清单
# 都参与取最大值；远端账本不可读时宁可停止，也不能复用已打包的版本号。
REPOSITORY=$(gh repo view --json nameWithOwner --jq '.nameWithOwner')
CANDIDATES=("$(node -p "require('./package.json').version")")
while IFS= read -r tag; do
  CANDIDATES+=("${tag#v}")
done < <(git tag --list 'v[0-9]*')

ASSET_ID=$(gh api "repos/${REPOSITORY}/releases/tags/nightly" \
  --jq '.assets[] | select(.name == "nightly.json") | .id')
if [[ -z "$ASSET_ID" ]]; then
  echo "✗ Nightly Release 缺少 nightly.json，无法确定全局下一版本。" >&2
  exit 1
fi
NIGHTLY_VERSION=$(gh api -H 'Accept: application/octet-stream' \
  "repos/${REPOSITORY}/releases/assets/${ASSET_ID}" --jq '.version')
CANDIDATES+=("$NIGHTLY_VERSION")

NEXT_VERSION=$(node scripts/next-build-version.mjs "$BUMP" "${CANDIDATES[@]}")
echo "全局下一版本: $NEXT_VERSION"
RELEASE_NOTES_FILE="$REPO_ROOT/release-notes/v${NEXT_VERSION}.json"
if [[ ! -f "$RELEASE_NOTES_FILE" ]]; then
  echo "✗ 缺少稳定版发布说明 $RELEASE_NOTES_FILE，请先整理双语更新内容并提交。" >&2
  exit 1
fi
node scripts/release-notes.mjs validate "$RELEASE_NOTES_FILE" "$NEXT_VERSION"
pnpm version "$NEXT_VERSION"

if [[ "$LOCAL_BUILD" -eq 1 ]]; then
  # 本地自测构建：自签产物，仅用于本机验收，不进任何分发渠道
  pnpm tauri build --bundles app --config "{\"bundle\":{\"macOS\":{\"signingIdentity\":\"$SIGN_ID\"}}}"
  bash scripts/bundle-tray.sh
  SIGN_ID="$SIGN_ID" RELEASE_NOTES_FILE="$RELEASE_NOTES_FILE" src-widget/build.sh
else
  echo "✓ v${NEXT_VERSION} 已 bump 并打 tag。分发产物由 Release CI 构建，后续："
  echo "  1. monet-maintainer prepare"
  echo "  2. git push --atomic origin main refs/tags/v${NEXT_VERSION}"
  echo "  （本地自测构建可用 pnpm release -- --local-build）"
fi

#!/bin/bash
# 发版：升版本号 → 打包 → Helper App → Widget
# 用法：
#   pnpm release            → 全局下一 patch（同时计入现有 Nightly）
#   pnpm release -- minor   → 基于全局最大版本升 minor
#   pnpm release -- major   → 基于全局最大版本升 major

set -euo pipefail

BUMP=${1:-patch}
SIGN_ID=${SIGN_ID:-Monet Signing}

if [[ "$BUMP" != "patch" && "$BUMP" != "minor" && "$BUMP" != "major" ]]; then
  echo "✗ 版本升级类型只支持 patch、minor 或 major。" >&2
  exit 1
fi

# 工作区必须干净：不代替用户决定提交内容（并行开发时盲提交会混入无关改动）
if ! git diff --quiet || ! git diff --cached --quiet || [ -n "$(git ls-files --others --exclude-standard)" ]; then
  echo "✗ 工作区有未提交改动，请先自行提交（按主题切分）后再发版：" >&2
  git status --short >&2
  exit 1
fi

IDENTITIES=$(security find-identity -v -p codesigning)
if ! grep -F "\"$SIGN_ID\"" <<< "$IDENTITIES" >/dev/null; then
  echo "✗ 找不到代码签名身份 '$SIGN_ID'，已停止发版。" >&2
  exit 1
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
pnpm version "$NEXT_VERSION"
pnpm tauri build --bundles app --config "{\"bundle\":{\"macOS\":{\"signingIdentity\":\"$SIGN_ID\"}}}"
bash scripts/bundle-tray.sh
SIGN_ID="$SIGN_ID" src-widget/build.sh

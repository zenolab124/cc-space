#!/bin/bash
# 发版：升版本号 → 打包 → Helper App → Widget
# 用法：
#   pnpm release            → patch (0.1.0 → 0.1.1)
#   pnpm release -- minor   → minor (0.1.0 → 0.2.0)
#   pnpm release -- major   → major (0.1.0 → 1.0.0)

set -euo pipefail

BUMP=${1:-patch}
SIGN_ID=${SIGN_ID:-Monet Signing}

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

pnpm version "$BUMP"
pnpm tauri build --bundles app --config "{\"bundle\":{\"macOS\":{\"signingIdentity\":\"$SIGN_ID\"}}}"
bash scripts/bundle-tray.sh
SIGN_ID="$SIGN_ID" src-widget/build.sh

#!/bin/bash
# 先把完整资产上传到隐藏候选 Release 并校验，再切换 nightly。
# 切换阶段任一步失败都会恢复旧 tag / Release，避免更新通道被清空。
set -euo pipefail

: "${VERSION:?VERSION is required}"
: "${GITHUB_SHA:?GITHUB_SHA is required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
: "${GITHUB_RUN_ID:?GITHUB_RUN_ID is required}"
: "${GITHUB_RUN_ATTEMPT:?GITHUB_RUN_ATTEMPT is required}"

shopt -s nullglob
ASSETS=(
    src-tauri/target/release/bundle/dmg/*.dmg
    src-tauri/target/release/bundle/updater/*.app.tar.gz
    src-tauri/target/release/bundle/updater/*.sig
    src-tauri/target/release/bundle/updater/nightly.json
)
if [ "${#ASSETS[@]}" -ne 4 ]; then
    echo "Error: expected exactly four Nightly assets, found ${#ASSETS[@]}" >&2
    exit 1
fi

CANDIDATE_TAG="nightly-candidate-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}"
BACKUP_TAG="nightly-previous-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}"
RELEASE_TITLE="Nightly $VERSION"
RELEASE_NOTES="每日构建，含未经验证的改动。仅供在设置中切到 Nightly 通道的用户使用。提交 ${GITHUB_SHA:0:7}"
CANDIDATE_ID=""
OLD_RELEASE_ID=""
OLD_SHA=""
OLD_RENAMED=0
NIGHTLY_TAG_MOVED=0
CANDIDATE_PUBLISHED=0

release_id_for_tag() {
    local tag="$1"
    gh api --paginate "repos/$GITHUB_REPOSITORY/releases" \
        --jq ".[] | select(.tag_name == \"$tag\") | .id" | head -1
}

verify_release_assets() {
    local release_id="$1"
    local remote_count
    remote_count=$(gh api "repos/$GITHUB_REPOSITORY/releases/$release_id" --jq '.assets | length')
    if [ "$remote_count" -ne "${#ASSETS[@]}" ]; then
        echo "Error: candidate asset count mismatch" >&2
        return 1
    fi

    local asset name expected actual
    for asset in "${ASSETS[@]}"; do
        name=$(basename "$asset")
        expected="sha256:$(shasum -a 256 "$asset" | awk '{print $1}')"
        actual=$(gh api "repos/$GITHUB_REPOSITORY/releases/$release_id" \
            --jq ".assets[] | select(.name == \"$name\") | .digest")
        if [ -z "$actual" ] || [ "$actual" != "$expected" ]; then
            echo "Error: candidate digest mismatch for $name" >&2
            return 1
        fi
    done
}

rollback() {
    local status=$?
    trap - ERR
    set +e
    echo "Nightly switch failed; restoring the previous release" >&2

    if [ -z "$CANDIDATE_ID" ]; then
        CANDIDATE_ID=$(release_id_for_tag "$CANDIDATE_TAG" 2>/dev/null || true)
    fi

    if [ "$CANDIDATE_PUBLISHED" -eq 1 ] && [ -n "$CANDIDATE_ID" ]; then
        gh api --method PATCH \
            "repos/$GITHUB_REPOSITORY/releases/$CANDIDATE_ID" \
            -f tag_name="$CANDIDATE_TAG" -F draft=true -F prerelease=true >/dev/null
    fi
    if [ "$NIGHTLY_TAG_MOVED" -eq 1 ] && [ -n "$OLD_SHA" ]; then
        git tag -f nightly "$OLD_SHA"
        git push --force origin refs/tags/nightly >/dev/null
    fi
    if [ "$OLD_RENAMED" -eq 1 ] && [ -n "$OLD_RELEASE_ID" ]; then
        gh api --method PATCH \
            "repos/$GITHUB_REPOSITORY/releases/$OLD_RELEASE_ID" \
            -f tag_name=nightly -F draft=false -F prerelease=true >/dev/null
    fi
    if [ -n "$CANDIDATE_ID" ]; then
        gh api --method DELETE \
            "repos/$GITHUB_REPOSITORY/releases/$CANDIDATE_ID" >/dev/null
    fi
    git push origin ":refs/tags/$CANDIDATE_TAG" >/dev/null 2>&1 || true
    git push origin ":refs/tags/$BACKUP_TAG" >/dev/null 2>&1 || true
    exit "$status"
}
trap rollback ERR

# 候选 Release 在 draft 状态下完成全部上传与摘要校验，对更新用户不可见。
gh release create "$CANDIDATE_TAG" \
    --target "$GITHUB_SHA" \
    --draft \
    --prerelease \
    --title "$RELEASE_TITLE" \
    --notes "$RELEASE_NOTES" \
    "${ASSETS[@]}"
CANDIDATE_ID=$(release_id_for_tag "$CANDIDATE_TAG")
test -n "$CANDIDATE_ID"
verify_release_assets "$CANDIDATE_ID"

OLD_RELEASE_ID=$(release_id_for_tag nightly)
if [ -n "$OLD_RELEASE_ID" ]; then
    OLD_SHA=$(git rev-parse 'refs/tags/nightly^{commit}')
    git tag -f "$BACKUP_TAG" "$OLD_SHA"
    git push origin "refs/tags/$BACKUP_TAG" >/dev/null
    OLD_RENAMED=1
    gh api --method PATCH \
        "repos/$GITHUB_REPOSITORY/releases/$OLD_RELEASE_ID" \
        -f tag_name="$BACKUP_TAG" >/dev/null
fi

git tag -f nightly "$GITHUB_SHA"
NIGHTLY_TAG_MOVED=1
git push --force origin refs/tags/nightly >/dev/null

CANDIDATE_PUBLISHED=1
gh api --method PATCH \
    "repos/$GITHUB_REPOSITORY/releases/$CANDIDATE_ID" \
    -f tag_name=nightly \
    -f target_commitish="$GITHUB_SHA" \
    -f name="$RELEASE_TITLE" \
    -f body="$RELEASE_NOTES" \
    -F draft=false \
    -F prerelease=true >/dev/null

PUBLIC_RELEASE_ID=$(gh api "repos/$GITHUB_REPOSITORY/releases/tags/nightly" --jq '.id')
test "$PUBLIC_RELEASE_ID" = "$CANDIDATE_ID"
verify_release_assets "$PUBLIC_RELEASE_ID"

# 新入口已经完整可用。旧 Release 与临时 tag 的清理失败不应反向破坏新版。
trap - ERR
if [ -n "$OLD_RELEASE_ID" ]; then
    gh api --method DELETE \
        "repos/$GITHUB_REPOSITORY/releases/$OLD_RELEASE_ID" >/dev/null || \
        echo "Warning: failed to remove previous Nightly release" >&2
fi
git push origin ":refs/tags/$BACKUP_TAG" >/dev/null 2>&1 || \
    echo "Warning: failed to remove previous Nightly tag" >&2
git push origin ":refs/tags/$CANDIDATE_TAG" >/dev/null 2>&1 || true

echo "Nightly $VERSION published with verified assets"

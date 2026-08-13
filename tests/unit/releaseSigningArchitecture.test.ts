import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('macOS release signing architecture', () => {
  it('notarizes and staples the final app before packaging distributable artifacts', () => {
    const build = source('../../src-widget/build.sh')
    const finalAppSign = build.indexOf('--entitlements "$MAIN_ENTITLEMENTS" "$APP_BUNDLE"')
    const appNotarization = build.indexOf('submit_for_notarization "$NOTARY_ZIP"')
    const dmgCreation = build.indexOf('echo "=> Creating DMG..."')
    const dmgNotarization = build.indexOf('submit_for_notarization "$DMG_PATH"')
    const updaterCreation = build.indexOf('echo "=> Creating updater artifacts..."')

    expect(finalAppSign).toBeGreaterThan(-1)
    expect(appNotarization).toBeGreaterThan(finalAppSign)
    expect(build.indexOf('stapler staple "$APP_BUNDLE"')).toBeGreaterThan(appNotarization)
    expect(dmgCreation).toBeGreaterThan(appNotarization)
    expect(dmgNotarization).toBeGreaterThan(dmgCreation)
    expect(build.indexOf('stapler staple "$DMG_PATH"')).toBeGreaterThan(dmgNotarization)
    expect(updaterCreation).toBeGreaterThan(dmgNotarization)
    expect(build).toContain('EXPECTED_AUTHORITY="$SIGN_ID"')
    expect(build).toContain('^[A-Fa-f0-9]{40}$')
    expect(build).toContain('CODESIGN_ARGS+=(--timestamp)')
    expect(build).toContain('IDENTITY_LINE=$(grep -F "$SIGN_ID"')
    expect(build).toContain('for BIN in "$APP_BUNDLE/Contents/MacOS/"*')
    expect(build).toContain('"$PLUGINS_DIR/MonetWidgetExtension.appex"')
    expect(build).toContain('"$APP_BUNDLE/Contents/Library/LoginItems/MonetTray.app"')

    const afterAppStaple = build.slice(build.indexOf('stapler staple "$APP_BUNDLE"'))
    const laterAppMutation = afterAppStaple
      .split('\n')
      .some(line => /^\s*codesign\b/.test(line))
    expect(laterAppMutation).toBe(false)
    expect(build.match(/submit_for_notarization /g)).toHaveLength(2)
    expect(build).toContain('--wait || submit_status=$?')
  })

  it('keeps release secrets scoped to their exact signing operations', () => {
    const build = source('../../src-widget/build.sh')
    const secretCapture = build.indexOf('NOTARY_PRIVATE_KEY="${APPLE_API_PRIVATE_KEY:-}"')
    const secretScrub = build.indexOf('unset APPLE_API_PRIVATE_KEY APPLE_API_KEY_PATH')
    const firstBuildTool = build.indexOf('xcodegen generate')
    const keyMaterialization = build.indexOf('printf \'%s\' "$NOTARY_PRIVATE_KEY"')
    const notaryInvocation = build.indexOf('xcrun notarytool submit "$artifact"')
    const keyRemoval = build.indexOf('rm -f "$NOTARY_KEY_FILE"', notaryInvocation)
    const signerInjection = build.indexOf('TAURI_SIGNING_PRIVATE_KEY="$UPDATER_SIGNING_PRIVATE_KEY"')

    expect(secretCapture).toBeGreaterThan(-1)
    expect(secretScrub).toBeGreaterThan(secretCapture)
    expect(firstBuildTool).toBeGreaterThan(secretScrub)
    expect(keyMaterialization).toBeGreaterThan(secretScrub)
    expect(notaryInvocation).toBeGreaterThan(keyMaterialization)
    expect(keyRemoval).toBeGreaterThan(notaryInvocation)
    expect(signerInjection).toBeGreaterThan(build.indexOf('echo "=> Creating updater artifacts..."'))
    expect(build).not.toContain('--key "$APPLE_API_KEY_PATH"')

    for (const workflowPath of [
      '../../.github/workflows/nightly.yml',
      '../../.github/workflows/release.yml',
    ]) {
      const workflow = source(workflowPath)
      const buildApp = workflow.indexOf('name: Build app')
      const finalPackage = workflow.indexOf('name: Build widget, sign, package')

      expect(workflow).toContain('APPLE_API_PRIVATE_KEY: ${{ secrets.APPLE_API_PRIVATE_KEY }}')
      expect(workflow).toContain('APPLE_API_KEY: ${{ vars.APPLE_API_KEY }}')
      expect(workflow).toContain('APPLE_API_ISSUER: ${{ vars.APPLE_API_ISSUER }}')
      expect(finalPackage).toBeGreaterThan(buildApp)
      expect(workflow.indexOf('APPLE_API_PRIVATE_KEY:', buildApp)).toBeGreaterThan(finalPackage)
      expect(workflow).toContain('exec src-widget/build.sh')
      expect(workflow).not.toContain('AuthKey_${APPLE_API_KEY}.p8')
      expect(workflow).toContain("trap 'rm -f \"$CERT_FILE\"' EXIT")
      expect(workflow).toContain('unset CERT_P12 CERT_PASSWORD')
    }
  })

  it('pins every workflow action to an immutable commit SHA', () => {
    for (const workflowPath of [
      '../../.github/workflows/nightly.yml',
      '../../.github/workflows/release.yml',
    ]) {
      const workflow = source(workflowPath)
      const uses = [...workflow.matchAll(/^\s*-?\s*uses:\s*\S+@([^\s#]+)/gm)].map(match => match[1])
      expect(uses.length).toBeGreaterThan(0)
      expect(uses.every(ref => /^[a-f0-9]{40}$/.test(ref))).toBe(true)
    }
  })

  it('publishes a verified Nightly candidate and can restore the previous release', () => {
    const workflow = source('../../.github/workflows/nightly.yml')
    const publisher = source('../../scripts/publish-nightly.sh')
    const candidateUpload = publisher.indexOf('gh release create "$CANDIDATE_TAG"')
    const candidateVerification = publisher.indexOf('verify_release_assets "$CANDIDATE_ID"')
    const oldReleaseRename = publisher.indexOf('-f tag_name="$BACKUP_TAG"')
    const candidatePublish = publisher.indexOf('-f tag_name=nightly', oldReleaseRename)
    const publicVerification = publisher.indexOf('verify_release_assets "$PUBLIC_RELEASE_ID"')
    const oldReleaseDelete = publisher.lastIndexOf('repos/$GITHUB_REPOSITORY/releases/$OLD_RELEASE_ID')

    expect(workflow).toContain('bash scripts/publish-nightly.sh')
    expect(workflow).not.toContain('gh release delete nightly')
    expect(publisher).toContain('trap rollback ERR')
    expect(publisher).toContain('restoring the previous release')
    expect(candidateUpload).toBeGreaterThan(-1)
    expect(candidateVerification).toBeGreaterThan(candidateUpload)
    expect(oldReleaseRename).toBeGreaterThan(candidateVerification)
    expect(candidatePublish).toBeGreaterThan(oldReleaseRename)
    expect(publicVerification).toBeGreaterThan(candidatePublish)
    expect(oldReleaseDelete).toBeGreaterThan(publicVerification)
    expect(publisher).toContain('expected="sha256:$(shasum -a 256')
  })
})

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
    const appNotarization = build.indexOf('notarytool submit "$NOTARY_ZIP"')
    const dmgCreation = build.indexOf('echo "=> Creating DMG..."')
    const dmgNotarization = build.indexOf('notarytool submit "$DMG_PATH"')
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
  })

  it('keeps the notarization private key scoped to the final packaging step', () => {
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
    }
  })
})

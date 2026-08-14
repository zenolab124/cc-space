import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const card = readFileSync('src/components/artifacts/ArtifactPreviewCard.vue', 'utf8')
const backend = readFileSync('src-tauri/src/artifact_preview.rs', 'utf8')

describe('artifact preview security boundary', () => {
  it('keeps generated HTML in an opaque, scriptless iframe', () => {
    expect(card).toContain('sandbox=""')
    expect(card).not.toContain('allow-same-origin')
    expect(card).not.toContain('allow-scripts')
    expect(card).toContain('"script-src \'none\'"')
    expect(card).toContain('"connect-src \'none\'"')
    expect(card).toContain('referrerpolicy="no-referrer"')
    expect(card).toContain("querySelectorAll('meta[http-equiv]')")
    expect(card).toContain("querySelectorAll('a, area')")
    expect(card).toContain("querySelectorAll('base')")
  })

  it('canonicalizes both roots and files before containment checks', () => {
    expect(backend).toContain('let root = root')
    expect(backend).toContain('.canonicalize()')
    expect(backend).toContain('if !candidate.starts_with(&root)')
  })
})

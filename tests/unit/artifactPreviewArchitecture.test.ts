import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { clampArtifactFrameHeight } from '@/features/artifact-preview/sandboxHtml'

const card = readFileSync('src/components/artifacts/ArtifactPreviewCard.vue', 'utf8')
const sandbox = readFileSync('src/features/artifact-preview/sandboxHtml.ts', 'utf8')
const backend = readFileSync('src-tauri/src/artifact_preview.rs', 'utf8')
const blockText = readFileSync('src/components/blocks/BlockText.vue', 'utf8')

describe('artifact preview security boundary', () => {
  it('allows only the nonce-bound Monet measurement bridge in an opaque iframe', () => {
    expect(card).toContain('sandbox="allow-scripts"')
    expect(card).not.toContain('allow-same-origin')
    expect(sandbox).toContain("querySelectorAll('script')")
    expect(sandbox).toContain("startsWith('on')")
    expect(sandbox).toContain("`script-src 'nonce-${nonce}'`")
    expect(sandbox).toContain('new ResizeObserver(schedule)')
    expect(sandbox).toContain('setTimeout(() => requestAnimationFrame(measure), 100)')
    expect(sandbox).toContain("parent.postMessage({ type, token, height, fillsViewport }, '*')")
    expect(sandbox).toContain('"connect-src \'none\'"')
    expect(card).toContain('referrerpolicy="no-referrer"')
    expect(card).toContain('event.source !== frame.contentWindow')
    expect(card).toContain('data.token !== sandboxNonce.value')
    expect(sandbox).toContain("querySelectorAll('meta[http-equiv]')")
    expect(sandbox).toContain("querySelectorAll('a, area')")
    expect(sandbox).toContain("querySelectorAll('base')")
  })

  it('caps HTML at a portrait 3:4 frame and disposes offscreen payloads', () => {
    expect(sandbox).toContain('frameWidth) * 4 / 3')
    expect(card).toContain("rootMargin: '200px 0px'")
    expect(card).toContain('}, 500)')
    expect(card).toContain('artifact.value = null')
    expect(clampArtifactFrameHeight(900, 600)).toBe(800)
    expect(clampArtifactFrameHeight(100, 600)).toBe(240)
    expect(clampArtifactFrameHeight(900, 500)).toBe(666)
    expect(clampArtifactFrameHeight(Number.POSITIVE_INFINITY, 600)).toBe(800)
    expect(card).toContain('Number.POSITIVE_INFINITY')
    expect(card).toContain('contentFillsViewport')
  })

  it('canonicalizes both roots and files before containment checks', () => {
    expect(backend).toContain('let root = root')
    expect(backend).toContain('.canonicalize()')
    expect(backend).toContain('if !candidate.starts_with(&root)')
  })

  it('opens Markdown file links only through the workspace-bound command', () => {
    expect(blockText).toContain("invoke('open_workspace_file', { root, path })")
    expect(backend).toContain('pub fn open_workspace_file(root: String, path: String)')
    expect(backend).toContain('resolve_workspace_file(Path::new(&root), Path::new(&path))?')
  })
})

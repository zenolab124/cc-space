import { describe, expect, it } from 'vitest'
import {
  detectArtifactCandidates,
  normalizeArtifactLink,
} from '@/features/artifact-preview/detectArtifacts'

describe('normalizeArtifactLink', () => {
  it('accepts relative and absolute artifact paths', () => {
    expect(normalizeArtifactLink('./output/demo.html')).toBe('./output/demo.html')
    expect(normalizeArtifactLink('/workspace/output/demo.svg')).toBe('/workspace/output/demo.svg')
  })

  it('removes Codex line suffixes and URL encoding', () => {
    expect(normalizeArtifactLink('/workspace/my%20demo.html:12')).toBe('/workspace/my demo.html')
  })

  it('rejects remote and unsupported links', () => {
    expect(normalizeArtifactLink('https://example.com/demo.html')).toBeNull()
    expect(normalizeArtifactLink('javascript:alert(1)')).toBeNull()
    expect(normalizeArtifactLink('./output/demo.ts')).toBeNull()
  })
})

describe('detectArtifactCandidates', () => {
  it('prefers explicit Markdown file links over file-change guesses', () => {
    const result = detectArtifactCandidates(
      ['已生成 [预览页面](./dist/demo.html)。'],
      [
        { path: './dist/demo.html', changeKind: 'add' },
        { path: './dist/extra.svg', changeKind: 'add' },
      ],
    )

    expect(result).toEqual([{ path: './dist/demo.html', kind: 'html', linked: true }])
  })

  it('ignores links inside fenced code blocks', () => {
    const result = detectArtifactCandidates(
      ['```md\n[not delivery](./demo.html)\n```'],
      [],
    )

    expect(result).toEqual([])
  })

  it('uses created files when the final response has no link', () => {
    const result = detectArtifactCandidates(
      ['文件已经生成。'],
      [
        { path: 'demo.html', tool: 'Write' },
        { path: 'poster.svg', changeKind: 'added' },
      ],
    )

    expect(result.map(candidate => candidate.path)).toEqual(['demo.html', 'poster.svg'])
    expect(result.every(candidate => !candidate.linked)).toBe(true)
  })

  it('does not guess between multiple edited files', () => {
    const result = detectArtifactCandidates(
      [],
      [
        { path: 'index.html', tool: 'Edit' },
        { path: 'logo.svg', tool: 'Edit' },
      ],
    )

    expect(result).toEqual([])
  })
})

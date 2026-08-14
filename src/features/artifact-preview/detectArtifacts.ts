import MarkdownIt from 'markdown-it'
import type { EngineSegment } from '@/engines/types'
import type { ContentBlock } from '@/types'

export type ArtifactKind = 'html' | 'svg' | 'gif' | 'image'

export interface ArtifactCandidate {
  path: string
  kind: ArtifactKind
  linked: boolean
}

export interface ArtifactFileEvidence {
  path: string
  tool?: string | null
  changeKind?: string | null
}

const markdown = new MarkdownIt({ html: false, linkify: false })
const ARTIFACT_EXTENSION_RE = /\.(html?|svg|gif|png|jpe?g|webp)$/i
const LINE_SUFFIX_RE = /(\.(?:html?|svg|gif|png|jpe?g|webp)):\d+(?::\d+)?$/i
const LOCATION_SUFFIX_RE = /:\d+(?::\d+)?$/
const REMOTE_SCHEME_RE = /^(?:https?|data|javascript|blob):/i

export function artifactKind(path: string): ArtifactKind | null {
  const extension = path.match(ARTIFACT_EXTENSION_RE)?.[1]?.toLowerCase()
  if (!extension) return null
  if (extension === 'html' || extension === 'htm') return 'html'
  if (extension === 'svg') return 'svg'
  if (extension === 'gif') return 'gif'
  return 'image'
}

function decodePath(value: string): string {
  try {
    return decodeURIComponent(value)
  } catch (_) {
    return value
  }
}

export function normalizeLocalFileLink(value: string): string | null {
  let path = value.trim().replace(/^<|>$/g, '')
  if (!path || REMOTE_SCHEME_RE.test(path) || path.startsWith('#')) return null
  if (/^file:/i.test(path)) {
    try {
      path = new URL(path).pathname
    } catch (_) {
      return null
    }
  } else if (/^[a-z][a-z0-9+.-]*:/i.test(path) && !/^[a-z]:[\\/]/i.test(path)) {
    return null
  }
  path = decodePath(path.split(/[?#]/, 1)[0] ?? '')
  path = path.replace(LOCATION_SUFFIX_RE, '')
  return path || null
}

export function normalizeArtifactLink(value: string): string | null {
  let path = normalizeLocalFileLink(value)
  if (!path) return null
  path = path.replace(LINE_SUFFIX_RE, '$1')
  return path && artifactKind(path) ? path : null
}

function markdownArtifactLinks(text: string): string[] {
  const links: string[] = []
  const visit = (tokens: ReturnType<MarkdownIt['parse']>) => {
    for (const token of tokens) {
      if (token.type === 'link_open' || token.type === 'image') {
        const href = token.attrGet(token.type === 'image' ? 'src' : 'href')
        const path = href ? normalizeArtifactLink(href) : null
        if (path) links.push(path)
      }
      if (token.children) visit(token.children)
    }
  }
  visit(markdown.parse(text, {}))
  return links
}

function dedupeCandidates(paths: readonly string[], linked: boolean): ArtifactCandidate[] {
  const seen = new Set<string>()
  const result: ArtifactCandidate[] = []
  for (const path of paths) {
    const normalized = normalizeArtifactLink(path)
    if (!normalized) continue
    const key = normalized.replace(/\\/g, '/').toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    result.push({ path: normalized, kind: artifactKind(normalized)!, linked })
  }
  return result
}

/**
 * 最终回复里的标准 Markdown 文件链接是强信号；没有链接时，仅展示明确的创建/Write
 * 证据，或本轮唯一一个可预览文件。它不扫描文件系统，也不从普通文本猜路径。
 */
export function detectArtifactCandidates(
  texts: readonly string[],
  files: readonly ArtifactFileEvidence[],
): ArtifactCandidate[] {
  const linked = dedupeCandidates(texts.flatMap(markdownArtifactLinks), true)
  if (linked.length > 0) return linked.slice(0, 8)

  const eligible = files.filter(file => artifactKind(normalizeArtifactLink(file.path) ?? ''))
  const strong = eligible.filter(file => {
    const tool = file.tool?.toLowerCase()
    const change = file.changeKind?.toLowerCase()
    return tool === 'write' || change === 'add' || change === 'added' || change === 'create' || change === 'created'
  })
  const fallback = strong.length > 0 ? strong : eligible.length === 1 ? eligible : []
  return dedupeCandidates(fallback.map(file => file.path), false).slice(0, 8)
}

export function detectContentBlockArtifacts(blocks: readonly ContentBlock[]): ArtifactCandidate[] {
  const texts: string[] = []
  const files: ArtifactFileEvidence[] = []
  for (const block of blocks) {
    if (block.type === 'text' && typeof block.text === 'string') texts.push(block.text)
    if (block.type !== 'tool_use') continue
    const candidate = block as { name?: unknown; input?: unknown }
    const name = typeof candidate.name === 'string' ? candidate.name : ''
    const input = candidate.input && typeof candidate.input === 'object' && !Array.isArray(candidate.input)
      ? candidate.input as Record<string, unknown>
      : null
    const path = name === 'NotebookEdit' ? input?.notebook_path : (input?.file_path ?? input?.path)
    if (typeof path === 'string') files.push({ path, tool: name })
  }
  return detectArtifactCandidates(texts, files)
}

export function detectEngineSegmentArtifacts(segments: readonly EngineSegment[]): ArtifactCandidate[] {
  const texts: string[] = []
  const files: ArtifactFileEvidence[] = []
  for (const segment of segments) {
    if (segment.kind === 'text' && segment.phase !== 'progress') texts.push(segment.text)
    if (segment.kind === 'fileChange') {
      files.push(...segment.changes.map(change => ({ path: change.path, changeKind: change.kind })))
    }
    if (segment.kind === 'toolCall') {
      const input = segment.input && typeof segment.input === 'object' && !Array.isArray(segment.input)
        ? segment.input as Record<string, unknown>
        : null
      const path = input?.file_path ?? input?.path
      if (typeof path === 'string') files.push({ path, tool: segment.name })
    }
  }
  return detectArtifactCandidates(texts, files)
}

export function artifactFileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path
}

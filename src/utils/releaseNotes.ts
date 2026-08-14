export type ReleaseNoteType = 'new' | 'improved' | 'fixed'
export type ReleaseChannel = 'stable' | 'nightly'

export interface ReleaseNoteItem {
  type: ReleaseNoteType
  title: string
  detail?: string
}

export interface LocalizedReleaseNotes {
  summary: string
  items: ReleaseNoteItem[]
}

export interface ParsedReleaseNotes {
  version?: string
  content: LocalizedReleaseNotes
  structured: boolean
}

interface ReleaseNotesEnvelope {
  schema: 1
  version: string
  locales: Record<string, LocalizedReleaseNotes>
}

const NOTE_TYPES = new Set<ReleaseNoteType>(['new', 'improved', 'fixed'])
const MAX_RAW_LENGTH = 32 * 1024
const MAX_ITEMS = 20

function cleanText(value: unknown, maxLength: number): string | null {
  if (typeof value !== 'string') return null
  const text = value.trim()
  if (!text || text.length > maxLength) return null
  return text
}

function parseLocalizedContent(value: unknown): LocalizedReleaseNotes | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const candidate = value as { summary?: unknown; items?: unknown }
  const summary = cleanText(candidate.summary, 240)
  if (!summary || !Array.isArray(candidate.items) || candidate.items.length === 0) return null

  const items: ReleaseNoteItem[] = []
  for (const rawItem of candidate.items.slice(0, MAX_ITEMS)) {
    if (!rawItem || typeof rawItem !== 'object' || Array.isArray(rawItem)) return null
    const item = rawItem as { type?: unknown; title?: unknown; detail?: unknown }
    if (typeof item.type !== 'string' || !NOTE_TYPES.has(item.type as ReleaseNoteType)) return null
    const title = cleanText(item.title, 180)
    if (!title) return null
    const detail = item.detail == null || item.detail === '' ? undefined : cleanText(item.detail, 500)
    if (item.detail != null && item.detail !== '' && !detail) return null
    items.push({ type: item.type as ReleaseNoteType, title, ...(detail ? { detail } : {}) })
  }
  return { summary, items }
}

function localeCandidates(locale: string): string[] {
  const language = locale.toLowerCase().split('-')[0]
  if (language === 'zh') return [locale, 'zh-CN', 'en-US']
  return [locale, 'en-US', 'zh-CN']
}

function parseStructured(raw: string, locale: string, expectedVersion?: string): ParsedReleaseNotes | null {
  let value: unknown
  try {
    value = JSON.parse(raw)
  } catch {
    return null
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const envelope = value as Partial<ReleaseNotesEnvelope>
  if (envelope.schema !== 1 || typeof envelope.version !== 'string' || !envelope.locales) return null
  if (expectedVersion && envelope.version !== expectedVersion) return null

  for (const candidate of localeCandidates(locale)) {
    const content = parseLocalizedContent(envelope.locales[candidate])
    if (content) return { version: envelope.version, content, structured: true }
  }
  return null
}

function parsePlainText(raw: string): ParsedReleaseNotes | null {
  const text = raw
    .replace(/<[^>]*>/g, '')
    .replace(/^\s{0,3}(?:#{1,6}\s*|[-*+]\s+)/gm, '')
    .trim()
    .slice(0, 4000)
  if (!text) return null
  return {
    content: { summary: text, items: [] },
    structured: false,
  }
}

/**
 * Updater notes 来自远端清单，只解析受限 JSON 并以 Vue 文本节点渲染。
 * 旧清单的普通文本会安全降级；不进入允许原始 HTML 的会话 Markdown 管线。
 */
export function parseReleaseNotes(raw: string, locale: string, expectedVersion?: string): ParsedReleaseNotes | null {
  if (!raw || raw.length > MAX_RAW_LENGTH) return null
  if (raw.trimStart().startsWith('{')) return parseStructured(raw, locale, expectedVersion)
  return parsePlainText(raw)
}

export function releaseNotesUrl(version: string, channel: ReleaseChannel): string {
  const tag = channel === 'nightly' ? 'nightly' : `v${version}`
  return `https://github.com/zenolab124/monet/releases/tag/${encodeURIComponent(tag)}`
}

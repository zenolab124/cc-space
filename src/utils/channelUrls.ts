export type ChannelAdapter = 'claude-code' | 'codex'

function normalizeBaseUrl(value: string): string {
  return value.trim().replace(/\/+$/, '')
}

function parsedUrl(value: string): URL | null {
  try {
    return new URL(value)
  } catch {
    return null
  }
}

function pathEndsWith(value: string, suffix: string): boolean {
  return parsedUrl(value)?.pathname.replace(/\/+$/, '').toLowerCase().endsWith(suffix) ?? false
}

function replacePath(value: string, transform: (path: string) => string): string {
  const parsed = parsedUrl(value)
  if (!parsed) return value
  parsed.pathname = transform(parsed.pathname.replace(/\/+$/, '')) || '/'
  const rendered = parsed.toString()
  return parsed.pathname === '/' && !parsed.search && !parsed.hash
    ? rendered.replace(/\/$/, '')
    : rendered
}

function stripPathSuffix(value: string, suffix: string): string {
  return replacePath(value, path => path.slice(0, -suffix.length).replace(/\/+$/, ''))
}

function appendPath(value: string, suffix: string): string {
  const normalized = normalizeBaseUrl(value)
  return replacePath(normalized, path => `${path}${suffix}`)
}

export function adapterBaseUrlCandidates(value: string, adapter: ChannelAdapter): string[] {
  let base = normalizeBaseUrl(value)
  if (!base) return []

  if (adapter === 'claude-code') {
    if (pathEndsWith(base, '/v1/messages')) base = stripPathSuffix(base, '/v1/messages')
    const candidates: string[] = []
    if (pathEndsWith(base, '/v1')) candidates.push(stripPathSuffix(base, '/v1'))
    if (!candidates.includes(base)) candidates.push(base)
    return candidates
  }

  if (pathEndsWith(base, '/responses')) base = stripPathSuffix(base, '/responses')
  if (pathEndsWith(base, '/v1')) {
    return [...new Set([base, stripPathSuffix(base, '/v1')])]
  }
  return [...new Set([appendPath(base, '/v1'), base])]
}

export function preferredAdapterBaseUrl(value: string, adapter: ChannelAdapter): string {
  return adapterBaseUrlCandidates(value, adapter)[0] ?? ''
}

export function adapterEndpointUrl(baseUrl: string, adapter: ChannelAdapter): string {
  const suffix = adapter === 'codex' ? '/responses' : '/v1/messages'
  return appendPath(baseUrl, suffix)
}

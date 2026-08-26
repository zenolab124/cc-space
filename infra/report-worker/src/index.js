/**
 * Monet public Issue relay.
 *
 * POST /report files anonymous diagnostics.
 * POST /theme-submission accepts only a strict declarative theme and files it
 * for maintainer review. The token remains limited to Issues read/write.
 */

const MAX_TITLE = 200
const MAX_BODY = 20_000
const REPORT_COOLDOWN_SECONDS = 600
const THEME_COOLDOWN_SECONDS = 1800

const COLOR_KEYS = [
  'background', 'foreground', 'card', 'cardForeground', 'popover', 'popoverForeground',
  'primary', 'primaryForeground', 'secondary', 'secondaryForeground', 'muted',
  'mutedForeground', 'accent', 'accentForeground', 'destructive', 'destructiveForeground',
  'border', 'input', 'ring', 'claude', 'codex', 'tag', 'tagForeground', 'visualBorder',
  'visualWarm', 'visualCool', 'visualRed', 'visualGreen',
]
const THEME_KEYS = [
  'schemaVersion', 'id', 'name', 'author', 'description', 'version', 'appearance',
  'source', 'colors', 'metrics', 'atmosphere',
]

const json = (data, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'content-type': 'application/json' },
  })

function exactKeys(value, allowed) {
  return value && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).length === allowed.length
    && Object.keys(value).every(key => allowed.includes(key))
}

function safeText(value, max) {
  if (typeof value !== 'string') return false
  const text = value.trim()
  const lower = text.toLowerCase()
  return text.length > 0 && text.length <= max
    && !/[<>\u0000-\u001f]/.test(text)
    && !text.includes('```')
    && !lower.includes('://')
    && !lower.includes('data:')
    && !lower.includes('base64')
    && !lower.includes('file:')
    && !text.startsWith('/')
    && !text.startsWith('\\\\')
    && !/^[a-z]:[\\/]/i.test(text)
}

function rgb(hex) {
  if (typeof hex !== 'string' || !/^#[0-9a-f]{6}$/i.test(hex)) return null
  return [1, 3, 5].map(offset => Number.parseInt(hex.slice(offset, offset + 2), 16))
}

function luminance(color) {
  const channels = rgb(color)
  if (!channels) return null
  const linear = channels.map(value => {
    const channel = value / 255
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
}

function contrast(a, b) {
  const first = luminance(a)
  const second = luminance(b)
  if (first === null || second === null) return 0
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05)
}

function validNumber(value, min, max) {
  return typeof value === 'number' && Number.isFinite(value) && value >= min && value <= max
}

function validateTheme(theme) {
  if (!exactKeys(theme, THEME_KEYS)) return 'theme fields do not match schema'
  if (theme.schemaVersion !== 1) return 'unsupported schemaVersion'
  if (typeof theme.id !== 'string' || !/^(?!-)[a-z0-9-]{3,40}(?<!-)$/.test(theme.id) || ['paper', 'ink'].includes(theme.id)) return 'invalid theme id'
  if (!safeText(theme.name, 60) || !safeText(theme.author, 60) || !safeText(theme.description, 500) || !safeText(theme.version, 20)) return 'invalid theme metadata'
  if (!['light', 'dark'].includes(theme.appearance)) return 'invalid appearance'
  if (!exactKeys(theme.source, ['kind']) || theme.source.kind !== 'local') return 'invalid source'
  if (!exactKeys(theme.colors, COLOR_KEYS) || !COLOR_KEYS.every(key => rgb(theme.colors[key]))) return 'invalid color tokens'
  if (!exactKeys(theme.metrics, ['radius', 'fontScale', 'lineHeight', 'shadow'])) return 'invalid metrics'
  if (!Number.isInteger(theme.metrics.radius) || !validNumber(theme.metrics.radius, 2, 16)) return 'invalid radius'
  if (!validNumber(theme.metrics.fontScale, 0.9, 1.15) || !validNumber(theme.metrics.lineHeight, 1.3, 1.9)) return 'invalid font metrics'
  if (!exactKeys(theme.metrics.shadow, ['color', 'opacity', 'y', 'blur']) || !rgb(theme.metrics.shadow.color)) return 'invalid shadow'
  if (!validNumber(theme.metrics.shadow.opacity, 0, 0.6) || !Number.isInteger(theme.metrics.shadow.y) || !validNumber(theme.metrics.shadow.y, 0, 16) || !Number.isInteger(theme.metrics.shadow.blur) || !validNumber(theme.metrics.shadow.blur, 0, 40)) return 'invalid shadow range'
  if (!exactKeys(theme.atmosphere, ['tint', 'noise', 'vignette']) || !rgb(theme.atmosphere.tint)) return 'invalid atmosphere'
  if (!validNumber(theme.atmosphere.noise, 0, 0.12) || !validNumber(theme.atmosphere.vignette, 0, 0.3)) return 'invalid atmosphere range'
  const pairs = [
    ['foreground', 'background'], ['cardForeground', 'card'], ['popoverForeground', 'popover'],
    ['primaryForeground', 'primary'], ['secondaryForeground', 'secondary'],
    ['mutedForeground', 'muted'], ['accentForeground', 'accent'],
    ['destructiveForeground', 'destructive'], ['tagForeground', 'tag'],
  ]
  if (pairs.some(([foreground, background]) => contrast(theme.colors[foreground], theme.colors[background]) < 4.5)) return 'theme contrast is below WCAG AA'
  if (JSON.stringify(theme).length > 15_000) return 'theme payload is too large'
  return null
}

function markdownText(value) {
  return value.replace(/[\\\[\]*_#]/g, match => `\\${match}`)
}

function themeIssue(theme, publicName) {
  const author = safeText(publicName, 60) ? publicName.trim() : 'Anonymous Monet user'
  const body = [
    '<!-- monet-theme-submission:v1 -->',
    `# Theme submission: ${markdownText(theme.name)}`,
    '',
    `- Author: ${markdownText(author)}`,
    `- Appearance: ${theme.appearance}`,
    `- Theme ID: \`${theme.id}\``,
    '',
    markdownText(theme.description),
    '',
    '## Machine-readable theme',
    '',
    '```json',
    JSON.stringify(theme, null, 2),
    '```',
    '',
    '---',
    '— Submitted anonymously through the Monet theme relay.',
    '',
  ].join('\n')
  return { title: `[Theme] ${theme.name}`, body }
}

async function cooldownKey(req, route) {
  const ip = req.headers.get('cf-connecting-ip') ?? 'unknown'
  const key = new Request(`https://cooldown.invalid/${route}/${encodeURIComponent(ip)}`)
  if (await caches.default.match(key)) return null
  return key
}

function markCooldown(ctx, key, seconds) {
  ctx.waitUntil(caches.default.put(key, new Response('1', { headers: { 'cache-control': `max-age=${seconds}` } })))
}

async function createIssue(env, title, body, labels) {
  const response = await fetch(`https://api.github.com/repos/${env.REPO}/issues`, {
    method: 'POST',
    headers: {
      authorization: `Bearer ${env.GITHUB_TOKEN}`,
      accept: 'application/vnd.github+json',
      'user-agent': 'monet-report-worker',
      'x-github-api-version': '2022-11-28',
    },
    body: JSON.stringify({ title, body, labels }),
  })
  if (!response.ok) return { error: `upstream error ${response.status}` }
  const issue = await response.json()
  return { url: issue.html_url }
}

async function handleReport(req, env, ctx, payload) {
  if (payload.website) return json({ ok: true, url: '' })
  const title = String(payload.title ?? '').trim()
  const body = String(payload.body ?? '').trim()
  const contact = String(payload.contact ?? '').trim()
  if (!title || !body) return json({ ok: false, error: 'title and body are required' }, 422)
  if (title.length > MAX_TITLE || body.length > MAX_BODY) return json({ ok: false, error: `limits: title ${MAX_TITLE} chars, body ${MAX_BODY} chars` }, 413)
  const key = await cooldownKey(req, 'report')
  if (!key) return json({ ok: false, error: 'rate limited — one report per 10 minutes' }, 429)
  const footer = ['', '---', contact && `Contact (voluntary): ${contact}`, '— Filed anonymously via AI diagnostics ([llms/troubleshoot.md](../blob/main/llms/troubleshoot.md))'].filter(Boolean).join('\n')
  const result = await createIssue(env, title, `${body}\n${footer}`, ['bug', 'ai-report'])
  if (result.error) return json({ ok: false, error: result.error }, 502)
  markCooldown(ctx, key, REPORT_COOLDOWN_SECONDS)
  return json({ ok: true, url: result.url })
}

async function handleTheme(req, env, ctx, payload) {
  if (payload.website) return json({ ok: true, url: '' })
  const validationError = validateTheme(payload.theme)
  if (validationError) return json({ ok: false, error: validationError }, 422)
  if (payload.publicName && !safeText(payload.publicName, 60)) return json({ ok: false, error: 'invalid publicName' }, 422)
  const key = await cooldownKey(req, 'theme')
  if (!key) return json({ ok: false, error: 'rate limited — one theme per 30 minutes' }, 429)
  const issue = themeIssue(payload.theme, payload.publicName)
  const result = await createIssue(env, issue.title, issue.body, ['theme-submission'])
  if (result.error) return json({ ok: false, error: result.error }, 502)
  markCooldown(ctx, key, THEME_COOLDOWN_SECONDS)
  return json({ ok: true, url: result.url })
}

export default {
  async fetch(req, env, ctx) {
    const url = new URL(req.url)
    if (req.method !== 'POST' || !['/report', '/theme-submission'].includes(url.pathname)) {
      return json({ ok: false, error: 'usage: POST /report or /theme-submission' }, 404)
    }
    let payload
    try { payload = await req.json() } catch { return json({ ok: false, error: 'invalid JSON' }, 400) }
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return json({ ok: false, error: 'JSON object required' }, 400)
    return url.pathname === '/report'
      ? handleReport(req, env, ctx, payload)
      : handleTheme(req, env, ctx, payload)
  },
}

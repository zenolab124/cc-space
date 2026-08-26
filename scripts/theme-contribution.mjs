import fs from 'node:fs'
import path from 'node:path'

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
  if (!exactKeys(theme, THEME_KEYS)) throw new Error('theme fields do not match schema')
  if (theme.schemaVersion !== 1) throw new Error('unsupported schemaVersion')
  if (typeof theme.id !== 'string' || !/^(?!-)[a-z0-9-]{3,40}(?<!-)$/.test(theme.id) || ['paper', 'ink'].includes(theme.id)) throw new Error('invalid theme id')
  if (!safeText(theme.name, 60) || !safeText(theme.author, 60) || !safeText(theme.description, 500) || !safeText(theme.version, 20)) throw new Error('invalid metadata')
  if (!['light', 'dark'].includes(theme.appearance)) throw new Error('invalid appearance')
  if (!exactKeys(theme.source, ['kind']) || theme.source.kind !== 'local') throw new Error('invalid source')
  if (!exactKeys(theme.colors, COLOR_KEYS) || !COLOR_KEYS.every(key => rgb(theme.colors[key]))) throw new Error('invalid colors')
  if (!exactKeys(theme.metrics, ['radius', 'fontScale', 'lineHeight', 'shadow'])) throw new Error('invalid metrics')
  if (!Number.isInteger(theme.metrics.radius) || !validNumber(theme.metrics.radius, 2, 16)) throw new Error('invalid radius')
  if (!validNumber(theme.metrics.fontScale, 0.9, 1.15) || !validNumber(theme.metrics.lineHeight, 1.3, 1.9)) throw new Error('invalid font metrics')
  const shadow = theme.metrics.shadow
  if (!exactKeys(shadow, ['color', 'opacity', 'y', 'blur']) || !rgb(shadow.color)
    || !validNumber(shadow.opacity, 0, 0.6) || !Number.isInteger(shadow.y)
    || !validNumber(shadow.y, 0, 16) || !Number.isInteger(shadow.blur)
    || !validNumber(shadow.blur, 0, 40)) throw new Error('invalid shadow')
  if (!exactKeys(theme.atmosphere, ['tint', 'noise', 'vignette']) || !rgb(theme.atmosphere.tint)
    || !validNumber(theme.atmosphere.noise, 0, 0.12)
    || !validNumber(theme.atmosphere.vignette, 0, 0.3)) throw new Error('invalid atmosphere')
  const pairs = [
    ['foreground', 'background'], ['cardForeground', 'card'], ['popoverForeground', 'popover'],
    ['primaryForeground', 'primary'], ['secondaryForeground', 'secondary'],
    ['mutedForeground', 'muted'], ['accentForeground', 'accent'],
    ['destructiveForeground', 'destructive'], ['tagForeground', 'tag'],
  ]
  if (pairs.some(([foreground, background]) => contrast(theme.colors[foreground], theme.colors[background]) < 4.5)) throw new Error('theme contrast is below WCAG AA')
  if (JSON.stringify(theme).length > 15_000) throw new Error('theme payload is too large')
}

function markdownText(value) {
  return value.replace(/[\\\[\]*_#]/g, match => `\\${match}`)
}

function xmlText(value) {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;')
}

function submittedAuthor(body, fallback) {
  const raw = body.match(/^- Author:\s*(.+)$/m)?.[1]?.replace(/\\([\\\[\]*_#])/g, '$1').trim()
  return safeText(raw, 60) ? raw : `@${fallback}`
}

function previewSvg(theme) {
  const colors = theme.colors
  return `<svg xmlns="http://www.w3.org/2000/svg" width="960" height="600" viewBox="0 0 960 600">
  <rect width="960" height="600" fill="${colors.background}"/>
  <rect x="36" y="36" width="176" height="528" rx="${theme.metrics.radius}" fill="${colors.secondary}" stroke="${colors.border}"/>
  <circle cx="72" cy="82" r="14" fill="${colors.primary}"/>
  <rect x="98" y="72" width="82" height="10" rx="5" fill="${colors.foreground}" opacity=".82"/>
  <rect x="64" y="132" width="116" height="9" rx="4.5" fill="${colors.mutedForeground}" opacity=".65"/>
  <rect x="64" y="166" width="98" height="9" rx="4.5" fill="${colors.mutedForeground}" opacity=".48"/>
  <rect x="244" y="36" width="680" height="528" rx="${theme.metrics.radius}" fill="${colors.card}" stroke="${colors.border}"/>
  <text x="284" y="98" fill="${colors.foreground}" font-family="system-ui, sans-serif" font-size="30" font-weight="650">${xmlText(theme.name)}</text>
  <text x="284" y="132" fill="${colors.mutedForeground}" font-family="system-ui, sans-serif" font-size="15">Deterministic Monet theme preview</text>
  <rect x="284" y="174" width="600" height="206" rx="${theme.metrics.radius}" fill="${colors.background}" stroke="${colors.border}"/>
  <text x="316" y="220" fill="${colors.primary}" font-family="system-ui, sans-serif" font-size="18" font-weight="600">Semantic colors</text>
  <rect x="316" y="250" width="116" height="42" rx="${theme.metrics.radius}" fill="${colors.primary}"/>
  <text x="344" y="277" fill="${colors.primaryForeground}" font-family="system-ui, sans-serif" font-size="14">Primary</text>
  <rect x="448" y="250" width="116" height="42" rx="${theme.metrics.radius}" fill="${colors.accent}"/>
  <text x="481" y="277" fill="${colors.accentForeground}" font-family="system-ui, sans-serif" font-size="14">Accent</text>
  <rect x="580" y="250" width="132" height="42" rx="${theme.metrics.radius}" fill="${colors.destructive}"/>
  <text x="604" y="277" fill="${colors.destructiveForeground}" font-family="system-ui, sans-serif" font-size="14">Destructive</text>
  <rect x="316" y="320" width="320" height="12" rx="6" fill="${colors.foreground}" opacity=".78"/>
  <rect x="316" y="346" width="430" height="9" rx="4.5" fill="${colors.mutedForeground}" opacity=".7"/>
  <rect x="284" y="418" width="286" height="104" rx="${theme.metrics.radius}" fill="${colors.visualWarm}" stroke="${colors.visualBorder}"/>
  <rect x="598" y="418" width="286" height="104" rx="${theme.metrics.radius}" fill="${colors.visualCool}" stroke="${colors.visualBorder}"/>
</svg>
`
}

const body = process.env.ISSUE_BODY ?? ''
const issueNumber = Number(process.env.ISSUE_NUMBER)
const issueUrl = process.env.ISSUE_URL ?? ''
const issueUser = process.env.ISSUE_USER ?? 'unknown'
if (!body.includes('<!-- monet-theme-submission:v1 -->')) throw new Error('theme marker missing')
if (!Number.isInteger(issueNumber) || issueNumber < 1) throw new Error('invalid issue number')
const jsonBlock = body.match(/## Machine-readable theme[\s\S]*?```json\s*([\s\S]*?)\s*```/i)?.[1]
if (!jsonBlock) throw new Error('machine-readable theme block missing')
const theme = JSON.parse(jsonBlock)
validateTheme(theme)

theme.author = submittedAuthor(body, issueUser)
theme.source = { kind: 'community', issue: issueNumber }

const themeDir = path.join('community-themes', theme.id)
const themeFile = path.join('src/themes/builtin', `${theme.id}.json`)
if (fs.existsSync(themeFile) || fs.existsSync(themeDir)) throw new Error(`theme id already exists: ${theme.id}`)
fs.mkdirSync('src/themes/builtin', { recursive: true })
fs.mkdirSync(themeDir, { recursive: true })
fs.writeFileSync(themeFile, `${JSON.stringify(theme, null, 2)}\n`)
fs.writeFileSync(path.join(themeDir, 'preview.svg'), previewSvg(theme))
fs.writeFileSync(path.join(themeDir, 'README.md'), `# ${markdownText(theme.name)}\n\n${markdownText(theme.description)}\n\n- Appearance: ${theme.appearance}\n- Author: ${markdownText(theme.author)}\n- Source: [Issue #${issueNumber}](${issueUrl})\n- Theme version: ${theme.version}\n`)

if (process.env.GITHUB_OUTPUT) {
  fs.appendFileSync(process.env.GITHUB_OUTPUT, `theme_id=${theme.id}\ntheme_name=${theme.name.replaceAll('\n', ' ')}\n`)
}

export type SemanticCopyMode = 'plain' | 'markdown' | 'rich' | 'full'

export interface SemanticClipboardPayload {
  plain: string
  markdown?: string
  html?: string
}

const COPY_EXCLUDE_SELECTOR = [
  '[data-copy-exclude]',
  '[hidden]',
  '[aria-hidden="true"]',
  '.hidden',
  '.code-copy-btn',
  '.sticky-nav-btn',
  '[style*="display: none"]',
  '[style*="display:none"]',
].join(',')

const SEMANTIC_TAGS = new Set([
  'a', 'abbr', 'article', 'blockquote', 'br', 'code', 'del', 'details', 'div', 'em',
  'figcaption', 'figure', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'img', 'li',
  'mark', 'ol', 'p', 'pre', 'section', 'span', 'strong', 'sub', 'summary', 'sup',
  'table', 'tbody', 'td', 'tfoot', 'th', 'thead', 'tr', 'ul',
])

const FULL_TAGS = new Set([
  ...SEMANTIC_TAGS,
  'aside', 'caption', 'col', 'colgroup', 'main', 'nav', 's', 'small', 'time',
])

const MATH_TAGS = new Set([
  'annotation', 'annotation-xml', 'math', 'menclose', 'merror', 'mfenced', 'mfrac',
  'mi', 'mmultiscripts', 'mn', 'mo', 'mover', 'mpadded', 'mphantom', 'mprescripts',
  'mroot', 'mrow', 'ms', 'mspace', 'msqrt', 'mstyle', 'msub', 'msubsup', 'msup',
  'mtable', 'mtd', 'mtext', 'mtr', 'munder', 'munderover', 'none', 'semantics',
])

const SAFE_STYLE_PROPERTIES = new Set([
  'align-items', 'background', 'background-color', 'border', 'border-color',
  'border-radius', 'border-style', 'border-width', 'color', 'column-count', 'columns',
  'display', 'flex', 'flex-direction', 'flex-wrap', 'font-style', 'font-weight', 'gap',
  'grid-template-columns', 'justify-content', 'line-height', 'margin', 'margin-bottom',
  'margin-left', 'margin-right', 'margin-top', 'max-width', 'min-width', 'padding',
  'padding-bottom', 'padding-left', 'padding-right', 'padding-top', 'text-align',
  'text-decoration', 'white-space', 'width',
])

const BLOCK_TAGS = new Set([
  'article', 'aside', 'blockquote', 'details', 'div', 'figcaption', 'figure', 'h1', 'h2',
  'h3', 'h4', 'h5', 'h6', 'hr', 'li', 'main', 'nav', 'ol', 'p', 'pre', 'section',
  'summary', 'table', 'tr', 'ul',
])

function normalizeNewlines(value: string): string {
  return value
    .replace(/\u00a0/g, ' ')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n[ \t]+/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

function normalizeOutput(value: string): string {
  return value
    .replace(/\u00a0/g, ' ')
    .replace(/[ \t]+\n/g, '\n')
    .trim()
}

function cleanFragment(fragment: DocumentFragment): HTMLDivElement {
  const container = document.createElement('div')
  container.append(fragment.cloneNode(true))
  container.querySelectorAll(COPY_EXCLUDE_SELECTOR).forEach(node => node.remove())
  container.querySelectorAll('button').forEach(button => {
    if (button.querySelector('img')) button.replaceWith(...button.childNodes)
    else button.remove()
  })
  container.querySelectorAll('details:not([open])').forEach(details => {
    for (const child of [...details.children]) {
      if (child.tagName.toLowerCase() !== 'summary') child.remove()
    }
  })
  normalizeMath(container)
  return container
}

function normalizeMath(container: HTMLElement) {
  for (const katex of [...container.querySelectorAll<HTMLElement>('.katex')]) {
    const annotation = katex.querySelector('annotation[encoding="application/x-tex"]')
    const math = katex.querySelector('math')
    const tex = annotation?.textContent?.trim()
    if (!tex) continue
    const replacement = document.createElement('span')
    replacement.dataset.copyTex = tex
    replacement.dataset.copyDisplay = katex.closest('.katex-display') ? 'block' : 'inline'
    if (math) replacement.append(math.cloneNode(true))
    katex.replaceWith(replacement)
  }
}

function isInternalImageUrl(value: string): boolean {
  return /^(?:data|blob|ccimg):/i.test(value)
    || /^https?:\/\/ccimg\.localhost(?:\/|$)/i.test(value)
    || /^https?:\/\/asset\.localhost(?:\/|$)/i.test(value)
}

function safeUrl(value: string | null, purpose: 'href' | 'src'): string | null {
  const url = value?.trim() ?? ''
  if (!url || /^(?:javascript|vbscript):/i.test(url)) return null
  if (purpose === 'src') {
    if (isInternalImageUrl(url)) return null
    if (/^[a-z][a-z\d+.-]*:/i.test(url) && !/^(?:https?|file):/i.test(url)) return null
    if (/^file:/i.test(url)) {
      try {
        return decodeURIComponent(new URL(url).pathname)
      } catch {
        return null
      }
    }
  }
  if (purpose === 'href' && /^(?:data|blob):/i.test(url)) return null
  return url
}

function reusableImageSource(element: Element): string | null {
  const explicitUrl = safeUrl(element.getAttribute('data-image-url'), 'src')
  if (explicitUrl) return explicitUrl
  const explicitPath = safeUrl(element.getAttribute('data-image-path'), 'src')
  if (explicitPath) return explicitPath
  return safeUrl(element.getAttribute('src'), 'src')
}

function escapeMarkdown(value: string): string {
  return value.replace(/([\\`*_[\]<>])/g, '\\$1')
}

function fenceFor(value: string): string {
  const runs = value.match(/`+/g) ?? []
  const longest = runs.reduce((max, run) => Math.max(max, run.length), 2)
  return '`'.repeat(longest + 1)
}

function markdownTable(table: Element): string {
  const rows = [...table.querySelectorAll(':scope > thead > tr, :scope > tbody > tr, :scope > tfoot > tr, :scope > tr')]
  if (!rows.length) return ''
  const cells = rows.map(row => [...row.children]
    .filter(cell => cell.matches('th,td'))
    .map(cell => normalizeNewlines(markdownChildren(cell)).replace(/\|/g, '\\|').replace(/\n/g, '<br>')))
  const width = Math.max(...cells.map(row => row.length))
  const header = cells[0]
  while (header.length < width) header.push('')
  const lines = [
    `| ${header.join(' | ')} |`,
    `| ${Array(width).fill('---').join(' | ')} |`,
  ]
  for (const row of cells.slice(1)) {
    while (row.length < width) row.push('')
    lines.push(`| ${row.join(' | ')} |`)
  }
  return `${lines.join('\n')}\n\n`
}

function markdownList(list: Element, depth = 0): string {
  const ordered = list.tagName.toLowerCase() === 'ol'
  const start = Number(list.getAttribute('start')) || 1
  const lines: string[] = []
  const items = [...list.children].filter(child => child.tagName.toLowerCase() === 'li')
  items.forEach((item, index) => {
    const nested = [...item.children].filter(child => child.matches('ul,ol'))
    const clone = item.cloneNode(true) as HTMLElement
    clone.querySelectorAll(':scope > ul, :scope > ol').forEach(child => child.remove())
    const prefix = ordered ? `${start + index}. ` : '- '
    const body = normalizeNewlines(markdownChildren(clone)).replace(/\n/g, `\n${'  '.repeat(depth + 1)}`)
    lines.push(`${'  '.repeat(depth)}${prefix}${body}`)
    for (const child of nested) lines.push(markdownList(child, depth + 1).trimEnd())
  })
  return `${lines.join('\n')}\n\n`
}

function markdownNode(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? ''
  if (!(node instanceof Element)) return ''
  const tex = node.getAttribute('data-copy-tex')
  if (tex) return node.getAttribute('data-copy-display') === 'block' ? `\n$$\n${tex}\n$$\n` : `$${tex}$`
  const tag = node.tagName.toLowerCase()
  if (tag === 'br') return '\n'
  if (tag === 'hr') return '\n---\n\n'
  if (tag === 'img') {
    const alt = node.getAttribute('alt')?.trim() || '图片'
    const source = reusableImageSource(node)
    return source ? `![${escapeMarkdown(alt)}](${source})` : `[图片：${alt}]`
  }
  if (tag === 'pre') {
    const value = node.textContent?.replace(/\n$/, '') ?? ''
    const fence = fenceFor(value)
    const language = node.querySelector('code')?.className.match(/language-([\w-]+)/)?.[1] ?? ''
    return `\n${fence}${language}\n${value}\n${fence}\n\n`
  }
  if (tag === 'code') {
    const value = node.textContent ?? ''
    const longest = (value.match(/`+/g) ?? []).reduce((max, run) => Math.max(max, run.length), 0)
    const fence = '`'.repeat(longest + 1)
    const padded = /^`|`$|^\s|\s$/.test(value) ? ` ${value} ` : value
    return `${fence}${padded}${fence}`
  }
  if (tag === 'strong' || tag === 'b') return `**${markdownChildren(node)}**`
  if (tag === 'em' || tag === 'i') return `*${markdownChildren(node)}*`
  if (tag === 'del' || tag === 's') return `~~${markdownChildren(node)}~~`
  if (tag === 'a') {
    const label = normalizeNewlines(markdownChildren(node))
    const href = safeUrl(node.getAttribute('href'), 'href')
    return href ? `[${label.replace(/\]/g, '\\]')}](${href})` : label
  }
  if (/^h[1-6]$/.test(tag)) return `\n${'#'.repeat(Number(tag[1]))} ${normalizeNewlines(markdownChildren(node))}\n\n`
  if (tag === 'blockquote') {
    const value = normalizeNewlines(markdownChildren(node))
    return `\n${value.split('\n').map(line => `> ${line}`).join('\n')}\n\n`
  }
  if (tag === 'ul' || tag === 'ol') return markdownList(node)
  if (tag === 'table') return markdownTable(node)
  const content = markdownChildren(node)
  return BLOCK_TAGS.has(tag) ? `\n${content}\n` : content
}

function markdownChildren(node: Node): string {
  return [...node.childNodes].map(markdownNode).join('')
}

function plainList(list: Element, depth = 0): string {
  const ordered = list.tagName.toLowerCase() === 'ol'
  const start = Number(list.getAttribute('start')) || 1
  const lines: string[] = []
  const items = [...list.children].filter(child => child.tagName.toLowerCase() === 'li')
  items.forEach((item, index) => {
    const nested = [...item.children].filter(child => child.matches('ul,ol'))
    const clone = item.cloneNode(true) as HTMLElement
    clone.querySelectorAll(':scope > ul, :scope > ol').forEach(child => child.remove())
    const prefix = ordered ? `${start + index}. ` : '• '
    lines.push(`${'  '.repeat(depth)}${prefix}${normalizeNewlines(plainChildren(clone))}`)
    for (const child of nested) lines.push(plainList(child, depth + 1).trimEnd())
  })
  return `${lines.join('\n')}\n`
}

function plainNode(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? ''
  if (!(node instanceof Element)) return ''
  const tex = node.getAttribute('data-copy-tex')
  if (tex) return node.getAttribute('data-copy-display') === 'block' ? `\n$$\n${tex}\n$$\n` : `$${tex}$`
  const tag = node.tagName.toLowerCase()
  if (tag === 'br') return '\n'
  if (tag === 'hr') return '\n——\n'
  if (tag === 'img') return `[图片：${node.getAttribute('alt')?.trim() || '图片'}]`
  if (tag === 'pre') return `\n${node.textContent?.replace(/\n$/, '') ?? ''}\n`
  if (tag === 'a') {
    const text = normalizeNewlines(plainChildren(node))
    const href = safeUrl(node.getAttribute('href'), 'href')
    return href && href !== text ? `${text} (${href})` : text
  }
  if (tag === 'ul' || tag === 'ol') return plainList(node)
  if (tag === 'table') {
    const rows = [...node.querySelectorAll('tr')].map(row => [...row.children]
      .filter(cell => cell.matches('th,td'))
      .map(cell => normalizeNewlines(plainChildren(cell)))
      .join('\t'))
    return `\n${rows.join('\n')}\n`
  }
  const content = plainChildren(node)
  return BLOCK_TAGS.has(tag) ? `\n${content}\n` : content
}

function plainChildren(node: Node): string {
  return [...node.childNodes].map(plainNode).join('')
}

function safeStyle(style: string): string {
  const declarations: string[] = []
  for (const declaration of style.split(';')) {
    const separator = declaration.indexOf(':')
    if (separator < 1) continue
    const property = declaration.slice(0, separator).trim().toLowerCase()
    const value = declaration.slice(separator + 1).trim()
    if (!SAFE_STYLE_PROPERTIES.has(property) || !value || value.length > 200) continue
    if (/(?:url\s*\(|expression\s*\(|javascript:|@import|-moz-binding)/i.test(value)) continue
    declarations.push(`${property}: ${value}`)
  }
  return declarations.join('; ')
}

function copySafeAttributes(source: Element, target: Element, full: boolean) {
  const tag = source.tagName.toLowerCase()
  if (tag === 'a') {
    const href = safeUrl(source.getAttribute('href'), 'href')
    if (href) target.setAttribute('href', href)
    if (/^https?:/i.test(href ?? '')) target.setAttribute('rel', 'noreferrer noopener')
  }
  if (tag === 'img') {
    const src = reusableImageSource(source)
    if (src) target.setAttribute('src', src)
    target.setAttribute('alt', source.getAttribute('alt')?.trim() || '图片')
    for (const name of ['width', 'height']) {
      const value = source.getAttribute(name)
      if (value && /^\d{1,5}$/.test(value)) target.setAttribute(name, value)
    }
  }
  for (const name of ['colspan', 'rowspan', 'start']) {
    const value = source.getAttribute(name)
    if (value && /^\d{1,4}$/.test(value)) target.setAttribute(name, value)
  }
  if (tag === 'details' && source.hasAttribute('open')) target.setAttribute('open', '')
  if (MATH_TAGS.has(tag)) {
    for (const name of ['display', 'encoding', 'mathvariant', 'stretchy', 'xmlns']) {
      const value = source.getAttribute(name)
      if (value && value.length <= 100) target.setAttribute(name, value)
    }
  }
  const tex = source.getAttribute('data-copy-tex')
  if (tex) target.setAttribute('data-tex', tex.slice(0, 10_000))
  if (full) {
    const authoredStyle = source.getAttribute('style') ?? ''
    let projectedStyle = ''
    if (source.isConnected && (source.hasAttribute('class') || authoredStyle)) {
      const computed = window.getComputedStyle(source)
      projectedStyle = [...SAFE_STYLE_PROPERTIES]
        .map(property => `${property}: ${computed.getPropertyValue(property)}`)
        .join('; ')
    }
    const style = safeStyle(`${authoredStyle};${projectedStyle}`)
    if (style) target.setAttribute('style', style)
    const title = source.getAttribute('title')
    if (title) target.setAttribute('title', title.slice(0, 500))
  }
}

function sanitizeNode(node: Node, full: boolean): Node | DocumentFragment | null {
  if (node.nodeType === Node.TEXT_NODE) return document.createTextNode(node.textContent ?? '')
  if (!(node instanceof Element)) return null
  const tag = node.tagName.toLowerCase()
  const allowed = MATH_TAGS.has(tag) || (full ? FULL_TAGS : SEMANTIC_TAGS).has(tag)
  if (!allowed) {
    const fragment = document.createDocumentFragment()
    for (const child of [...node.childNodes]) {
      const cleaned = sanitizeNode(child, full)
      if (cleaned) fragment.append(cleaned)
    }
    return fragment
  }
  if (tag === 'img' && !reusableImageSource(node)) {
    return document.createTextNode(`[图片：${node.getAttribute('alt')?.trim() || '图片'}]`)
  }
  const target = document.createElementNS(MATH_TAGS.has(tag) ? 'http://www.w3.org/1998/Math/MathML' : 'http://www.w3.org/1999/xhtml', tag)
  copySafeAttributes(node, target, full)
  for (const child of [...node.childNodes]) {
    const cleaned = sanitizeNode(child, full)
    if (cleaned) target.append(cleaned)
  }
  return target
}

function sanitizedHtml(container: HTMLElement, full: boolean): string {
  const staging = full ? document.createElement('div') : null
  if (staging) {
    staging.setAttribute('style', 'position: fixed; left: -100000px; top: 0; width: 720px; visibility: hidden; pointer-events: none')
    staging.append(container)
    document.body.append(staging)
  }
  const output = document.createElement('div')
  try {
    for (const child of [...container.childNodes]) {
      const cleaned = sanitizeNode(child, full)
      if (cleaned) output.append(cleaned)
    }
  } finally {
    staging?.remove()
  }
  return output.innerHTML
}

export function buildSemanticClipboardPayload(
  fragment: DocumentFragment,
  mode: SemanticCopyMode,
): SemanticClipboardPayload {
  const container = cleanFragment(fragment)
  const plain = normalizeOutput(plainChildren(container))
  if (mode === 'plain') return { plain }
  const markdown = normalizeOutput(markdownChildren(container))
  if (mode === 'markdown') return { plain: markdown, markdown }
  if (mode === 'rich') return { plain, html: sanitizedHtml(container, false) }
  const html = sanitizedHtml(container, true)
  // 全量复制的纯文本备用格式也必须是 HTML 源码：富文本目标消费 text/html，
  // 聊天框/代码编辑器等纯文本目标则能拿到完整标签与内联样式。
  return { plain: html, markdown, html }
}

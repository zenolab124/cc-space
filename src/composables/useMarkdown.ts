import MarkdownIt from 'markdown-it'
import { katex } from '@mdit/plugin-katex'
import markdownItShiki from '@shikijs/markdown-it'
import type { BundledLanguage } from 'shiki'
// 相对路径 + .ts 扩展名而非 @ 别名:scripts/bench/render-bench.mjs 用 node 直接 import 本模块,
// node 原生 ESM 不解析 vite 别名且要求显式扩展名
import { probeMd } from '../utils/perfProbe.ts'

const COPY_ICON = '<svg class="copy-icon" width="14" height="14" viewBox="0 0 32 32"><path fill="currentColor" d="M28 10v18H10V10zm0-2H10a2 2 0 0 0-2 2v18a2 2 0 0 0 2 2h18a2 2 0 0 0 2-2V10a2 2 0 0 0-2-2"/><path fill="currentColor" d="M4 18H2V4a2 2 0 0 1 2-2h14v2H4Z"/></svg>'
const CHECK_ICON = '<svg class="check-icon" width="14" height="14" viewBox="0 0 32 32"><path fill="currentColor" d="m13 24l-9-9l1.414-1.414L13 21.171L26.586 7.586L28 9z"/></svg>'
const COPY_BTN = `<button class="code-copy-btn" type="button">${COPY_ICON}${CHECK_ICON}</button>`

function wrapCodeBlocks(html: string): string {
  return html.replace(/<pre(\s[^>]*)?>[\s\S]*?<\/pre>/g, (m) =>
    `<div class="code-block-wrapper">${COPY_BTN}${m}</div>`,
  )
}

// 常用语言，按需加载
const LANGS: BundledLanguage[] = [
  'javascript', 'typescript', 'python', 'rust', 'go', 'java',
  'bash', 'shell', 'json', 'yaml', 'toml', 'html', 'css',
  'vue', 'jsx', 'tsx', 'sql', 'swift', 'kotlin', 'ruby',
  'c', 'cpp', 'diff', 'markdown', 'xml',
]

const mdOpts = { html: true, linkify: true, breaks: false, typographer: false }
const katexOpts = {
  delimiters: 'all' as const,
  throwOnError: false,
  trust: false,
}

function sanitizeHtml(html: string): string {
  return html
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, '')
    .replace(/<([a-z][a-z0-9]*)((?:\s+[^>]*?)?)>/gi, (_m, tag, attrs) => {
      const cleaned = (attrs as string).replace(/\bon\w+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]*)/gi, '')
      return cleaned !== attrs ? `<${tag}${cleaned}>` : _m
    })
}

function createMarkdownIt(): MarkdownIt {
  return new MarkdownIt(mdOpts).use(katex, katexOpts)
}

// 轻量渲染器:无 shiki,常驻。流式期间用它,代码块素色但 parse 成本低一个数量级
const plainMd = createMarkdownIt()

// 当前活跃的完整渲染器（非响应式，避免触发递归更新）;shiki 就绪前由轻量版兜底
let activeMd: MarkdownIt = plainMd
let shikiReady = false

// shiki 延迟初始化:原先在 module 加载时立即发起,启动期就同步拉 26 个语言文件 + 608KB WASM
// 并编译语法,抢占 mount 后的主线程(白屏打点里 1.1s 阻塞段的头号嫌疑)。改为首次真正需要高亮
// 渲染时才发起,给启动主线程让路;plainMd 兜底不变,shiki 就绪前照旧走素色渲染。
let shikiStarted = false

// 幂等:已发起(或已完成)直接返回,只有首次调用才真正拉起 shiki
function ensureShiki(): void {
  if (shikiStarted) return
  shikiStarted = true
  // boot:shiki-start / boot:shiki-ready 两个 mark 名是与 HUD 展示任务的契约,必须一字不差;
  // performance 在 node(render-bench.mjs 直接 import)也有 globalThis.performance,?. 兜个底
  performance?.mark?.('boot:shiki-start')
  markdownItShiki({
    themes: { light: 'github-light', dark: 'github-dark' },
    langs: LANGS,
    defaultColor: false,
    // 白名单外语言(nginx/ini/...)回退纯文本,否则 codeToHtml 抛 ShikiError 炸掉整个块渲染。
    // 'text' 是 shiki 运行时放行的 special language,不在 BundledLanguage 类型里,需断言
    fallbackLanguage: 'text' as BundledLanguage,
  }).then(plugin => {
    const md = createMarkdownIt()
    md.use(plugin)
    activeMd = md
    shikiReady = true
    performance?.mark?.('boot:shiki-ready')
  })
}

// 空闲预热:启动主线程让路后,借浏览器空闲档期提前把 shiki 备好,免得首次高亮渲染现拉现编。
// 仅浏览器执行——render-bench.mjs 用 node 直接 import 本模块(见文件头注释),node 无 window 故跳过;
// 老版 WebKit 有 window 但无 requestIdleCallback,typeof 检查后回退 setTimeout
if (typeof window !== 'undefined') {
  if (typeof requestIdleCallback !== 'undefined') {
    requestIdleCallback(() => ensureShiki(), { timeout: 5000 })
  } else {
    setTimeout(() => ensureShiki(), 3000)
  }
}

/** 流式降级渲染:跳过 shiki 高亮。流式中文本每帧变化,全量高亮是逐帧主线程大头 */
export function renderMarkdownPlain(text: string): string {
  const t0 = performance.now()
  const html = sanitizeHtml(wrapCodeBlocks(plainMd.render(text)))
  const dt = performance.now() - t0
  probeMd('plain', dt)
  // HUD 长帧归因埋点(生产常开,measure 开销微秒级)
  performance.measure('md-plain', { start: t0, duration: dt })
  return html
}

// 完成态渲染缓存:key 为原文,LRU。shiki 输出用 CSS 变量双主题,HTML 不随亮暗切换变,可安全缓存。
// 上限为字节预算而非条数(PRD v2.5.0 FR-005):段组件化后条目粒度变小、数量变多,
// 且换树预热的全文条目不能被小条目挤出;估算口径 UTF-16 每字符 2 字节,key+value 都计
const CACHE_BYTE_BUDGET = 32 * 1024 * 1024
let cacheBytes = 0
const htmlCache = new Map<string, string>()

function cacheEntryBytes(text: string, html: string): number {
  return (text.length + html.length) * 2
}

function cacheSet(text: string, html: string): void {
  const existing = htmlCache.get(text)
  if (existing !== undefined) {
    cacheBytes -= cacheEntryBytes(text, existing)
    htmlCache.delete(text)
  }
  htmlCache.set(text, html)
  cacheBytes += cacheEntryBytes(text, html)
  while (cacheBytes > CACHE_BYTE_BUDGET && htmlCache.size > 1) {
    const oldest = htmlCache.keys().next().value!
    cacheBytes -= cacheEntryBytes(oldest, htmlCache.get(oldest)!)
    htmlCache.delete(oldest)
  }
}

/** 带缓存的完整渲染:用于内容不再变化的块(历史消息、流式结束后的块) */
export function renderMarkdownCached(text: string): string {
  // 完成态渲染是唯一需要高亮的入口,借它兜住空闲预热的漏网(首帧就要高亮时立即拉起 shiki)
  ensureShiki()
  const hit = htmlCache.get(text)
  if (hit !== undefined) {
    // 命中移到队尾,维持 LRU 序(Map 迭代序 = 插入序)
    htmlCache.delete(text)
    htmlCache.set(text, hit)
    probeMd('hit', 0)
    return hit
  }
  const t0 = performance.now()
  const html = sanitizeHtml(wrapCodeBlocks(activeMd.render(text)))
  const dt = performance.now() - t0
  probeMd('miss', dt)
  performance.measure('md-shiki', { start: t0, duration: dt })
  // shiki 就绪前的结果是无高亮版,不入缓存,避免固化素色 HTML
  if (shikiReady) {
    cacheSet(text, html)
  }
  return html
}

// ---- 渐进 shiki 队列:流式结束时逐块上色,每帧处理一块,消除同帧 burst ----

const deferQueue: Array<{ text: string; resolve: (html: string) => void }> = []
let deferRunning = false

function drainDeferQueue() {
  if (deferQueue.length === 0) { deferRunning = false; return }
  deferRunning = true
  const { text, resolve } = deferQueue.shift()!
  resolve(renderMarkdownCached(text))
  if (deferQueue.length > 0) requestAnimationFrame(drainDeferQueue)
  else deferRunning = false
}

/** 异步 shiki 渲染:缓存命中则同步返回,未命中则排队每帧处理一块 */
export function renderMarkdownDeferred(text: string): Promise<string> {
  const hit = htmlCache.get(text)
  if (hit !== undefined) {
    htmlCache.delete(text)
    htmlCache.set(text, hit)
    probeMd('hit', 0)
    return Promise.resolve(hit)
  }
  return new Promise(resolve => {
    deferQueue.push({ text, resolve })
    if (!deferRunning) requestAnimationFrame(drainDeferQueue)
  })
}

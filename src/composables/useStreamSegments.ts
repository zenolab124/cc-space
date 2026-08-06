import { reactive, ref, watch, onUnmounted } from 'vue'
import { containsBlockHtml, createStreamSplitter } from '@/lib/stream-markdown/findSafeSplit'
import { renderMarkdownDeferred, renderMarkdownPlain } from './useMarkdown'

/**
 * 流式段状态机(PRD v2.5.0 FR-002/004/005 的逻辑核):
 * - 增量分割:安全分割点间隔 ≥MIN_SEGMENT_LEN 时冻结为段,冻结段永不回滚/改写
 * - 流式结束:含围栏的段逐帧回填 shiki 上色(位移面=单段);无围栏段 plain 即终态,跳过
 * - 产物单向:上色排队完成后预热「历史路径同 key」的完成态全文,换树时必命中缓存
 * 供流式出生的 BlockText 独占消费;历史出生的块不经过这里(模式单向)。
 */

export interface StreamSegment {
  source: string
  colored?: string
}

/** 段最小长度:小于该间隔的分割点跳过(相邻小段合并),防组件数爆炸 */
const MIN_SEGMENT_LEN = 200

const hasFence = (s: string) => s.includes('```') || s.includes('~~~')

export function useStreamSegments(opts: {
  /** 当前渲染文本(已经过 8K 截断策略) */
  text: () => string
  streaming: () => boolean
  /** 完成态历史路径将渲染的同一字符串(FR-005 预热 key,须与 renderMarkdownCached 入参逐字节一致) */
  persistText: () => string
}) {
  const segments = reactive<StreamSegment[]>([])
  const tailSource = ref('')
  const tailColored = ref<string | undefined>(undefined)
  // 块级 HTML 只能放在一个 v-html 根节点内。否则浏览器会分别修复各段的
  // 未闭合标签,出现样式尾巴、Markdown 管道符泄漏和重复节点。
  const atomicSource = ref<string | undefined>(undefined)
  const atomicColored = ref<string | undefined>(undefined)
  let atomicMode = false
  let splitter = createStreamSplitter()
  let frozenEnd = 0
  // 上色代际:rebuild/续流时 ++,在途回填回调按代际短路作废(FR-004 边界②)
  let generation = 0
  let disposed = false

  function ingest(text: string): void {
    if (atomicMode || containsBlockHtml(text)) {
      atomicMode = true
      atomicSource.value = text
      atomicColored.value = undefined
      segments.length = 0
      tailSource.value = ''
      tailColored.value = undefined
      frozenEnd = text.length
      return
    }
    const points = splitter.update(text)
    let start = frozenEnd
    for (const p of points) {
      if (p <= start || p - start < MIN_SEGMENT_LEN) continue
      segments.push({ source: text.slice(start, p) })
      start = p
    }
    frozenEnd = start
    tailSource.value = text.slice(frozenEnd)
  }

  /** 流式结束的素→彩:每段一个 defer 队列条目,每帧至多一段 DOM 被替换 */
  function colorize(): void {
    const gen = ++generation
    const atomicText = atomicSource.value
    const atomicReady = atomicText ? renderMarkdownDeferred(atomicText) : null
    if (atomicReady) {
      atomicReady.then(html => {
        if (disposed || gen !== generation) return
        atomicColored.value = html
      })
    }
    for (const seg of segments) {
      // plain 与 shiki 输出仅围栏渲染有差,无围栏段 plain 即终态,回填是无谓 DOM 动作
      if (seg.colored !== undefined || !hasFence(seg.source)) continue
      renderMarkdownDeferred(seg.source).then(html => {
        if (disposed || gen !== generation) return
        seg.colored = html
      })
    }
    const tailText = tailSource.value
    if (tailText && hasFence(tailText)) {
      renderMarkdownDeferred(tailText).then(html => {
        if (disposed || gen !== generation) return
        tailColored.value = html
      })
    }
    // FR-005 产物单向:排在段上色之后入队,预热完成态全文进 LRU;
    // deferredRecords 换树时历史区 renderMarkdownCached 必命中。
    // FR-006:预热排在全部段上色之后 resolve,天然是"上色完成"同步点,DEV 下借它做段拼接自查
    const persist = opts.persistText()
    const persistReady = atomicText && atomicText === persist
      ? atomicReady!
      : renderMarkdownDeferred(persist)
    persistReady.then(fullHtml => {
      if (!import.meta.env.DEV || disposed || gen !== generation) return
      // 展开态下段基于全文而 persist 是截断串,两者不可比,跳过
      if (opts.text() !== persist) return
      import('@/lib/stream-markdown/devConsistencyCheck').then(({ devCheckSegments }) => {
        if (disposed || gen !== generation) return
        const joined = atomicText
          ? (atomicText === persist ? fullHtml : (atomicColored.value ?? renderMarkdownPlain(atomicText)))
          : segments.map(s => s.colored ?? renderMarkdownPlain(s.source)).join('') +
            (tailColored.value ?? (tailSource.value ? renderMarkdownPlain(tailSource.value) : ''))
        devCheckSegments(persist, joined, fullHtml)
      })
    })
  }

  /** 展开/折叠切换:文本收缩违反分割器前缀契约,全量重建段状态(低频,允许整块重渲) */
  function rebuild(): void {
    generation++
    splitter = createStreamSplitter()
    segments.length = 0
    frozenEnd = 0
    atomicMode = false
    atomicSource.value = undefined
    atomicColored.value = undefined
    tailColored.value = undefined
    ingest(opts.text())
    if (!opts.streaming()) colorize()
  }

  watch(opts.text, t => {
    if (opts.streaming()) ingest(t)
  })

  watch(opts.streaming, (now, was) => {
    if (was && !now) colorize()
    else if (!was && now) {
      // 罕见同块续流:作废在途上色,已上色段保持彩色(内容不变不回退),tail 回素继续增量
      generation++
      tailColored.value = undefined
      atomicColored.value = undefined
    }
  })

  onUnmounted(() => { disposed = true })

  // 挂载时流式已在途(delta 已累积):先吞当前文本
  ingest(opts.text())

  return { segments, tailSource, tailColored, atomicSource, atomicColored, rebuild }
}

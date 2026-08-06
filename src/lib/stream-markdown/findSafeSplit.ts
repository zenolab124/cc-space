/**
 * 流式 markdown 安全分割器(增量扫描 + 分割守卫)
 *
 * 职责:对不断追加的流式 markdown 文本给出「安全分割点」——在这些位置切段后,
 * 逐段渲染再拼接的 HTML 与全文一次渲染语义等价。供 BlockText 段组件化消费(PRD v2.5.0 FR-003)。
 *
 * 增量契约:update(text) 的 text 必须是上次调用的前缀扩展(流式追加语义),
 * 扫描状态跨调用保留,每次只扫新增的完整行;末尾不完整的行不消费,留待下次。
 *
 * 守卫规则(从 streamdown parse-blocks 抄规则,不引其依赖):
 * ① 行首引用式链接定义 [label]: —— 整块永不分段(定义与引用可跨段,切开即断链)
 * ② 脚注 [^ —— 同上
 * ③ 候选点的段首行是列表标记或空白缩进 —— 跳过该点(松散列表切开会重置有序编号、断列表结构)
 * ④ 行首块级 HTML 标签未闭合(含 <!-- 注释)期间 —— 不分割(半个元素各渲染一半,结构不一致)
 * ⑤ 行首 $$ / \\[ 数学块未闭合期间 —— 不分割(防止 KaTeX 块公式被拆开)
 *
 * 守卫触发的代价只是「少分割」:段变长或整块走 tail,正确性优先于分段收益。
 */

const BLANK_RE = /^[ \t]*$/
const FENCE_RE = /^ {0,3}(`{3,}|~{3,})/
const LIST_MARKER_RE = /^ {0,3}(?:[-*+]|\d{1,9}[.)])[ \t]/
const REF_DEF_RE = /^ {0,3}\[[^\]]*\]:/
const MATH_DOLLAR_RE = /^ {0,3}\$\$/
const MATH_BRACKET_START_RE = /^ {0,3}\\\[/
const MATH_BRACKET_END_RE = /^ {0,3}\\\]/
const HTML_LINE_RE = /^ {0,3}<[a-zA-Z!/]/
const TAG_RE = /<(\/?)([a-zA-Z][a-zA-Z0-9-]*)(?:[^>]*?)(\/?)>/g
const VOID_TAGS = new Set([
  'area', 'base', 'br', 'col', 'embed', 'hr', 'img',
  'input', 'link', 'meta', 'param', 'source', 'track', 'wbr',
])

export interface StreamSplitter {
  /**
   * 喂入当前全文(须为上次调用的前缀扩展),返回已确认的安全分割点数组(字符偏移,升序)。
   * 分割点语义:该偏移是新段的段首(前段含其尾随空行)。返回数组为拷贝,可安全持有。
   */
  update(text: string): number[]
  /** 守卫①②触发:整块不分段,update 恒返回 [] */
  readonly blocked: boolean
}

export function createStreamSplitter(): StreamSplitter {
  let pos = 0                    // 已消费偏移,恒为行首
  let inFence = false
  let fenceMarker = ''           // '`' 或 '~'
  let fenceLen = 0
  let inMath = false
  let mathCloser: '$$' | '\\]' | null = null
  let inComment = false
  let htmlDepth = 0
  let blocked = false
  let prevLineBlank = false
  const confirmed: number[] = []

  /** 维护块级 HTML 开/闭标签深度(void 元素与自闭合不计) */
  function trackHtmlTags(line: string): void {
    TAG_RE.lastIndex = 0
    let m: RegExpExecArray | null
    while ((m = TAG_RE.exec(line)) !== null) {
      const [, closing, tag, selfClose] = m
      if (VOID_TAGS.has(tag.toLowerCase()) || selfClose === '/') continue
      if (closing === '/') htmlDepth = Math.max(0, htmlDepth - 1)
      else htmlDepth++
    }
  }

  function processLine(line: string, lineStart: number): void {
    // HTML 注释:未闭合期间吞掉一切判定
    if (inComment) {
      if (line.includes('-->')) inComment = false
      prevLineBlank = false
      return
    }

    // 代码围栏内:只找匹配的闭栏(CommonMark:闭栏同 marker 且长度 ≥ 开栏)
    if (inFence) {
      const m = FENCE_RE.exec(line)
      if (m && m[1][0] === fenceMarker && m[1].length >= fenceLen && BLANK_RE.test(line.slice(m[0].length))) {
        inFence = false
      }
      prevLineBlank = false
      return
    }

    // 数学块内:只找对应的闭合分隔符
    if (inMath) {
      if ((mathCloser === '$$' && MATH_DOLLAR_RE.test(line))
        || (mathCloser === '\\]' && MATH_BRACKET_END_RE.test(line))) {
        inMath = false
        mathCloser = null
      }
      prevLineBlank = false
      return
    }

    if (BLANK_RE.test(line)) {
      // 空行:候选信号。HTML 块深度归零前空行不解除守卫(多行元素内部允许空行)
      prevLineBlank = true
      return
    }

    // —— 至此为普通非空行,先做候选确认(此行即候选段的段首行) ——
    if (prevLineBlank && htmlDepth === 0) {
      const indented = line[0] === ' ' || line[0] === '\t'
      if (!indented && !LIST_MARKER_RE.test(line)) {
        confirmed.push(lineStart)
      }
      // 缩进/列表标记 → 守卫③跳过,不确认也不报错
    }
    prevLineBlank = false

    // —— 行级状态推进 ——
    // 守卫①②:引用式链接定义 / 脚注(定义或引用),一票整块封锁
    if (REF_DEF_RE.test(line) || line.includes('[^')) {
      blocked = true
      return
    }
    const fence = FENCE_RE.exec(line)
    if (fence) {
      inFence = true
      fenceMarker = fence[1][0]
      fenceLen = fence[1].length
      return
    }
    if (MATH_DOLLAR_RE.test(line)) {
      // 同行偶数个 $$(如 "$$x$$")视为已闭合
      const count = (line.match(/\$\$/g) ?? []).length
      if (count % 2 === 1) {
        inMath = true
        mathCloser = '$$'
      }
      return
    }
    if (MATH_BRACKET_START_RE.test(line) && !line.includes('\\]')) {
      inMath = true
      mathCloser = '\\]'
      return
    }
    if (htmlDepth > 0 || HTML_LINE_RE.test(line)) {
      if (line.includes('<!--') && !line.includes('-->')) {
        inComment = true
        return
      }
      trackHtmlTags(line)
    }
  }

  return {
    get blocked() { return blocked },
    update(text: string): number[] {
      if (blocked) return []
      // 只消费完整行;末尾不完整行留待下次(其内容可能还在流入,判定会翻转)
      while (!blocked) {
        const nl = text.indexOf('\n', pos)
        if (nl === -1) break
        processLine(text.slice(pos, nl), pos)
        pos = nl + 1
      }
      return blocked ? [] : confirmed.slice()
    },
  }
}

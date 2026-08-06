import { describe, it, expect } from 'vitest'
import { createStreamSplitter } from '../../src/lib/stream-markdown/findSafeSplit'

/** 一次性全文扫描(基准口径) */
function fullScan(text: string): number[] {
  return createStreamSplitter().update(text)
}

/** 按随机步长模拟流式追加,返回最终分割点(增量口径) */
function incrementalScan(text: string, seed = 42): { points: number[]; blocked: boolean } {
  // 简单 LCG 伪随机,保证用例可复现
  let s = seed
  const rand = () => (s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff
  const splitter = createStreamSplitter()
  let fed = 0
  let points: number[] = []
  while (fed < text.length) {
    fed = Math.min(text.length, fed + 1 + Math.floor(rand() * 50))
    points = splitter.update(text.slice(0, fed))
  }
  return { points, blocked: splitter.blocked }
}

describe('基本分割', () => {
  it('两个段落在空行处分割,分割点为次段段首', () => {
    expect(fullScan('para one\n\npara two\n')).toEqual([10])
  })

  it('多个连续空行:分割点仍是非空行行首(段尾吞掉全部空行)', () => {
    expect(fullScan('a\n\n\n\nb\n')).toEqual([5])
  })

  it('无空行则无分割点', () => {
    expect(fullScan('single paragraph\nwith soft break\n')).toEqual([])
  })

  it('空字符串与纯空白不 throw、无分割点', () => {
    expect(fullScan('')).toEqual([])
    expect(fullScan('   \n\t\n  \n')).toEqual([])
  })

  it('1MB 无换行单行不 throw、无分割点', () => {
    const big = 'x'.repeat(1024 * 1024)
    expect(fullScan(big)).toEqual([])
  })
})

describe('守卫①②:引用定义/脚注 → 整块封锁', () => {
  it('引用式链接定义触发 blocked', () => {
    const sp = createStreamSplitter()
    expect(sp.update('text\n\n[ref]: https://example.com\n\nmore\n')).toEqual([])
    expect(sp.blocked).toBe(true)
  })

  it('脚注定义与脚注引用均触发 blocked', () => {
    const def = createStreamSplitter()
    def.update('a\n\n[^1]: note\n')
    expect(def.blocked).toBe(true)

    const use = createStreamSplitter()
    use.update('see note[^1]\n\nb\n')
    expect(use.blocked).toBe(true)
  })

  it('普通行内链接不误伤', () => {
    const sp = createStreamSplitter()
    const pts = sp.update('see [foo](https://x.com) here\n\nnext para\n')
    expect(sp.blocked).toBe(false)
    expect(pts.length).toBe(1)
  })

  it('blocked 后即使继续追加也恒返回空', () => {
    const sp = createStreamSplitter()
    sp.update('[^1]: note\n')
    expect(sp.update('[^1]: note\n\nmore\n\nand more\n')).toEqual([])
  })
})

describe('守卫③:列表续行与缩进跳过', () => {
  it('松散列表项之间不分割(有序编号防重置)', () => {
    expect(fullScan('1. first\n\n2. second\n\n3. third\n')).toEqual([])
  })

  it('无序松散列表同样跳过', () => {
    expect(fullScan('- a\n\n- b\n')).toEqual([])
  })

  it('列表后接普通段落:该点确认分割', () => {
    const pts = fullScan('- a\n- b\n\nplain paragraph\n')
    expect(pts).toEqual(['- a\n- b\n\n'.length])
  })

  it('缩进续行(懒续/缩进代码)跳过', () => {
    expect(fullScan('- item\n\n    continuation\n')).toEqual([])
    expect(fullScan('para\n\n    indented code\n')).toEqual([])
  })
})

describe('围栏与数学块', () => {
  it('代码围栏内的空行不产生分割点', () => {
    expect(fullScan('```js\ncode\n\nmore code\n```\n\nafter\n')).toEqual(['```js\ncode\n\nmore code\n```\n\n'.length])
  })

  it('四反引号围栏内嵌三反引号不误闭合', () => {
    const text = '````\n```\ninner\n```\n````\n\nafter\n'
    expect(fullScan(text)).toEqual([text.indexOf('after')])
  })

  it('波浪线围栏 ~~~ 同样识别', () => {
    expect(fullScan('~~~\nx\n\ny\n~~~\n\nafter\n')).toEqual(['~~~\nx\n\ny\n~~~\n\n'.length])
  })

  it('围栏开栏行自身可作为新段段首', () => {
    const text = 'para\n\n```js\ncode\n```\n'
    expect(fullScan(text)).toEqual([6])
  })

  it('$$ 数学块未闭合期间不分割,闭合后恢复', () => {
    const text = '$$\nx = 1\n\ny = 2\n$$\n\nafter\n'
    expect(fullScan(text)).toEqual([text.indexOf('after')])
  })

  it('同行 $$…$$ 闭合不进入数学块状态', () => {
    expect(fullScan('$$x^2$$\n\nafter\n')).toEqual([9])
  })

  it('\\[…\\] 数学块未闭合期间不分割', () => {
    const text = '\\[\nx = 1\n\ny = 2\n\\]\n\nafter\n'
    expect(fullScan(text)).toEqual([text.indexOf('after')])
  })
})

describe('守卫④:HTML 块', () => {
  it('未闭合块级标签期间不分割,闭合后恢复', () => {
    const text = '<div>\n\ncontent inside\n\n</div>\n\nafter\n'
    expect(fullScan(text)).toEqual([text.indexOf('after')])
  })

  it('void 元素与自闭合标签不计深度', () => {
    const pts = fullScan('<br>\n\nnext\n')
    expect(pts).toEqual([6])
    expect(fullScan('<img src="x"/>\n\nnext\n')).toEqual(['<img src="x"/>\n\n'.length])
  })

  it('HTML 注释未闭合期间不分割', () => {
    const text = '<!--\nhidden\n\nstill hidden\n-->\n\nafter\n'
    expect(fullScan(text)).toEqual([text.indexOf('after')])
  })
})

describe('增量契约', () => {
  const SAMPLES = [
    'para one\n\npara two\n\npara three\n',
    '# title\n\n```ts\nconst a = 1\n\nconst b = 2\n```\n\n- l1\n\n- l2\n\ntail para\n',
    'a\n\n<div>\n\n</div>\n\nb\n\n1. x\n\n2. y\n\nz\n',
    '$$\nmath\n$$\n\ntext\n\n````\n```\n````\n\nend\n',
  ]

  it('随机步长增量扫描与一次性全文扫描结果一致', () => {
    for (const text of SAMPLES) {
      for (const seed of [1, 7, 42, 1337]) {
        expect(incrementalScan(text, seed).points, `seed=${seed} text=${JSON.stringify(text.slice(0, 30))}`)
          .toEqual(fullScan(text))
      }
    }
  })

  it('末尾不完整行不消费:后续补全后判定正确', () => {
    const sp = createStreamSplitter()
    // "1" 此刻看似普通段首,但可能是 "1. " 列表的前缀 —— 必须悬置
    expect(sp.update('para\n\n1')).toEqual([])
    expect(sp.update('para\n\n1. item\n')).toEqual([])   // 补全为列表 → 守卫③跳过
    const sp2 = createStreamSplitter()
    sp2.update('para\n\n1')
    expect(sp2.update('para\n\n1 plain text\n')).toEqual([6])  // 补全为普通段 → 确认
  })
})

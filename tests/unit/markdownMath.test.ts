import { describe, expect, it } from 'vitest'
import { renderMarkdownPlain } from '../../src/composables/useMarkdown'

describe('Markdown 数学公式', () => {
  it('渲染美元行内公式和块公式', () => {
    const html = renderMarkdownPlain('Inline $E = mc^2$.\n\n$$\n\\frac{1}{2}\n$$')

    expect(html).toContain('class="katex"')
    expect(html).toContain('katex-display')
  })

  it('支持 LaTeX 方括号分隔符', () => {
    const html = renderMarkdownPlain('Inline \\(x^2\\).\n\n\\[\\sum_{i=1}^n i\\]')

    expect(html).toContain('class="katex"')
    expect(html).toContain('katex-display')
  })

  it('不解析行内代码和代码块中的美元符号', () => {
    const html = renderMarkdownPlain('`$not math$`\n\n```text\n$also not math$\n```')

    expect(html).not.toContain('katex')
    expect(html).toContain('$not math$')
    expect(html).toContain('$also not math$')
  })

  it('公式语法错误时不抛出渲染异常', () => {
    expect(() => renderMarkdownPlain('$\\notARealCommand$')).not.toThrow()
  })
})

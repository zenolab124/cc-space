import { describe, expect, it } from 'vitest'
import { hasReportedUsage, shouldReplaceUsage, type TokenUsage } from '../../src/types'

const usage = (input = 0, output = 0): TokenUsage => ({
  input_tokens: input,
  output_tokens: output,
  cache_creation_input_tokens: 0,
  cache_read_input_tokens: 0,
})

describe('usage snapshot selection', () => {
  it('把零值占位替换成非零快照', () => {
    expect(shouldReplaceUsage(usage(), null, usage(100, 20), null)).toBe(true)
  })

  it('不让后续零值覆盖已报告用量', () => {
    expect(shouldReplaceUsage(usage(100, 20), null, usage(), 'end_turn')).toBe(false)
  })

  it('同为非零时优先终结快照', () => {
    expect(shouldReplaceUsage(usage(100, 10), null, usage(100, 20), 'tool_use')).toBe(true)
    expect(shouldReplaceUsage(usage(100, 20), 'tool_use', usage(100, 10), null)).toBe(false)
  })

  it('四个计费字段全零时视为尚未报告', () => {
    expect(hasReportedUsage(usage())).toBe(false)
    expect(hasReportedUsage(usage(1))).toBe(true)
  })
})

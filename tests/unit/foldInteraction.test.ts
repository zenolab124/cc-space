import { beforeEach, describe, expect, it, vi } from 'vitest'

const values = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
    clear: () => values.clear(),
  },
  configurable: true,
})

describe('fold interactions', () => {
  beforeEach(() => {
    values.clear()
    vi.resetModules()
  })

  it('keeps a normal thinking click local and makes Shift-click global and persistent', async () => {
    const { useThinkingExpand } = await import('@/composables/useThinkingExpand')
    const first = useThinkingExpand()
    const second = useThinkingExpand()

    first.toggle({ shiftKey: false })
    expect(first.thinkingExpanded.value).toBe(true)
    expect(second.thinkingExpanded.value).toBe(false)

    first.toggle({ shiftKey: true })
    expect(first.thinkingExpanded.value).toBe(false)
    expect(second.thinkingExpanded.value).toBe(false)
    expect(localStorage.getItem('monet:thinking-expanded')).toBe('0')

    second.toggle({ shiftKey: true })
    expect(first.thinkingExpanded.value).toBe(true)
    expect(second.thinkingExpanded.value).toBe(true)
    expect(useThinkingExpand().thinkingExpanded.value).toBe(true)
    expect(localStorage.getItem('monet:thinking-expanded')).toBe('1')
  })

  it('clears per-view tool overrides when a Shift-click changes a remembered default', async () => {
    const { createToolFoldState } = await import('@/composables/useToolDisplay')
    const first = createToolFoldState()
    const second = createToolFoldState()

    first.expandedGroups.add('first-group')
    second.collapsedGroups.add('second-group')
    first.setAllGroups(true)

    expect(first.groupDefaultExpanded.value).toBe(true)
    expect(second.groupDefaultExpanded.value).toBe(true)
    expect(first.expandedGroups.size).toBe(0)
    expect(second.collapsedGroups.size).toBe(0)
    expect(localStorage.getItem('monet:tool-group-expanded')).toBe('1')

    first.collapsedItems.add('first-item')
    second.expandedItems.add('second-item')
    second.setAllItems(true)

    expect(first.itemDefaultExpanded.value).toBe(true)
    expect(second.itemDefaultExpanded.value).toBe(true)
    expect(first.collapsedItems.size).toBe(0)
    expect(second.expandedItems.size).toBe(0)
    expect(localStorage.getItem('monet:tool-item-expanded')).toBe('1')
  })
})

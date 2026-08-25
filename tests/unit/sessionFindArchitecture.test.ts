import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('multi-engine session find architecture', () => {
  it('keeps the search request and status local to each workbench column', () => {
    const column = source('../../src/components/workbench/WorkbenchColumn.vue')
    const unified = source('../../src/components/session/UnifiedSessionDetail.vue')

    expect(column).toContain('const findQuery = ref')
    expect(column).toContain('@keydown.capture="onColumnKeydown"')
    expect(column).toContain(':find-request="findRequest"')
    expect(column).toContain('@find-status="onFindStatus"')
    expect(unified).toContain(':find-request="findRequest"')
    expect(unified).toContain("emit('findStatus', $event)")
  })

  it('indexes normalized user and assistant text for both detail controllers', () => {
    const nativeDetail = source('../../src/components/SessionDetail.vue')
    const engineDetail = source('../../src/components/engine/EngineSessionDetail.vue')

    expect(nativeDetail).toContain("block.type === 'text'")
    expect(engineDetail).toContain("record.role === 'user' || record.role === 'assistant'")
    expect(engineDetail).toContain("segment.kind === 'text'")
    for (const detail of [nativeDetail, engineDetail]) {
      expect(detail).toContain('useSessionFindNavigation(')
      expect(detail).toContain("'session-find-active'")
    }
  })

  it('navigates through virtualized history rather than searching mounted DOM only', () => {
    const nativeDetail = source('../../src/components/SessionDetail.vue')
    const engineDetail = source('../../src/components/engine/EngineSessionDetail.vue')

    expect(nativeDetail).toContain("messageVirtualizer.value.scrollToIndex(index, { align: 'center' })")
    expect(engineDetail).toContain("conversationVirtualizer.value.scrollToIndex(index, { align: 'center' })")
  })
})

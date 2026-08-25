import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('workbench recent-tab view architecture', () => {
  it('mounts only a bounded set of recently visited tabs', () => {
    const workbench = source('../../src/views/WorkbenchView.vue')

    expect(workbench).toContain('const RECENT_TAB_CACHE_LIMIT = 4')
    expect(workbench).toContain('v-for="tab in cachedTabs"')
    expect(workbench).toContain('v-show="tab.id === activeTab.id"')
    expect(workbench).toContain(':tab="tab"')
  })

  it('binds cached panes to their own tab instead of the global active tab', () => {
    const pane = source('../../src/components/workbench/WorkbenchTabPane.vue')
    const boundChildren = [
      '../../src/components/workbench/MonitorRail.vue',
      '../../src/components/workbench/WorkbenchColumns.vue',
      '../../src/components/workbench/RaceColumns.vue',
      '../../src/composables/useColumnResize.ts',
    ].map(source)

    expect(pane).toContain('<MonitorRail v-show="!monitorRailCollapsed" :tab="tab" />')
    expect(pane).toContain('<WorkbenchColumns :tab="tab" />')
    expect(pane).toContain('<RaceColumns v-else :tab="tab" />')
    for (const child of boundChildren) expect(child).not.toContain('activeTab')
  })
})

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('feature integration boundaries', () => {
  it('targets ordinary workbenches through a split button and remembers the last one', () => {
    const button = source('../../src/components/workbench/WorkbenchTargetButton.vue')
    const workbench = source('../../src/composables/useWorkbench.ts')

    expect(button).toContain('defaultOrdinaryTab')
    expect(button).toContain('ordinaryTabs.value.map')
    expect(button).toContain('openTarget(tab.id)')
    expect(button).toContain('v-if="!existing && ordinaryTabs.length"')
    expect(workbench).toContain('lastOrdinaryTabId')
    expect(workbench).toContain('tab.id === targetTabId && !tab.race')
  })

  it('keeps semantic copy modes distinct and sanitizes enhanced HTML', () => {
    const clipboard = source('../../src/utils/semanticClipboard.ts')

    expect(clipboard).toContain("export type SemanticCopyMode = 'plain' | 'markdown' | 'rich' | 'full'")
    expect(clipboard).toContain("if (mode === 'rich') return { plain, html: sanitizedHtml(container, false) }")
    expect(clipboard).toContain('return { plain, markdown, html: sanitizedHtml(container, true) }')
    expect(clipboard).toContain("javascript:|@import|-moz-binding")
    expect(clipboard).toContain("annotation[encoding=\"application/x-tex\"]")
    expect(clipboard).toContain("/^(?:data|blob|ccimg):/i")
  })

  it('allows MCP theme previews but reserves persistence for confirmed UI actions', () => {
    const mcp = source('../../src-tauri/src/bin/monet_mcp.rs')
    const manager = source('../../src/components/settings/ThemeManager.vue')
    const workflow = source('../../.github/workflows/theme-submission.yml')
    const worker = source('../../infra/report-worker/src/index.js')

    expect(mcp).toContain('"name": "theme_context"')
    expect(mcp).toContain('"name": "theme_preview"')
    expect(mcp).not.toContain('"name": "theme_save"')
    expect(manager).toContain("invoke<ThemeDefinition>('theme_save_preview'")
    expect(manager).toContain('currentPreview.value?.validation.valid')
    expect(manager).toContain('expectedBody: prepared.body')
    expect(workflow).toContain("github.event.label.name == 'theme-approved'")
    expect(worker).toContain("const validationError = validateTheme(payload.theme)")
    expect(worker).toContain("['theme-submission']")
  })
})

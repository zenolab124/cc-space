import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const source = readFileSync(
  fileURLToPath(new URL('../../src/composables/useWorkbench.ts', import.meta.url)),
  'utf8',
)

const rail = readFileSync(
  fileURLToPath(new URL('../../src/components/workbench/MonitorRail.vue', import.meta.url)),
  'utf8',
)

const column = readFileSync(
  fileURLToPath(new URL('../../src/components/workbench/WorkbenchColumn.vue', import.meta.url)),
  'utf8',
)

const picker = readFileSync(
  fileURLToPath(new URL('../../src/components/workbench/NewTaskEnginePicker.vue', import.meta.url)),
  'utf8',
)

const choicePanel = readFileSync(
  fileURLToPath(new URL('../../src/components/workbench/EngineChoicePanel.vue', import.meta.url)),
  'utf8',
)

describe('standard-engine workbench drafts', () => {
  it('keeps pre-message references within the runtime that created them', () => {
    expect(source).toContain('runtimeScope: string')
    expect(source).toContain('String(performance.timeOrigin)')
    expect(source).not.toContain('sessionStorage.getItem(ENGINE_DRAFT_SCOPE_KEY)')
    expect(source).toContain('runtimeScope: engineDraftRuntimeScope')
    expect(source).toContain('draft.runtimeScope !== engineDraftRuntimeScope')
    expect(source).toContain('removeSession(sid)')
  })

  it('remembers the channel already attached by thread creation', () => {
    expect(source).toContain('attachedChannel: string | null')
    expect(source).toContain("typeof draft.attachedChannel === 'string'")
  })

  it('can replace a blank runtime draft without changing its workbench position', () => {
    expect(source).toContain('function replaceWorkbenchSession')
    expect(source).toContain('column.sessionId = replacementSessionId')
    expect(source).toContain('tab.sessionIds[sessionIndex] = replacementSessionId')
    expect(source).toContain('teardownSession(sessionId)')
  })

  it('opens an engine-neutral task before creating either runtime', () => {
    expect(rail).toContain('createPendingTask(project.cwd)')
    expect(rail).not.toContain('<select')
    expect(rail).not.toContain('engineForProject')
    expect(source).toContain('pendingTasks: Record<string, string>')
    expect(source).toContain('function promotePendingTaskToDraft')
    expect(column).toContain('v-if="pendingTask"')
    expect(column).toContain('<NewTaskEnginePicker')
  })

  it('deduplicates the combined recent-project list by working directory', () => {
    expect(rail).toContain('const options = new Map<string, ProjectOption>()')
    expect(rail).toContain("cwd.replace(/[\\\\/]+$/, '')")
    expect(rail).toContain('projectOptions.value.slice(0, 5)')
  })

  it('keeps breathing room above the open-session action without shifting its popover anchor', () => {
    expect(rail).toContain('class="shrink-0 pt-2"')
    expect(rail).toContain('<div class="relative">')
    expect(rail).toContain('class="absolute bottom-full left-0 right-0 mb-1.5 z-50')
  })

  it('promotes Claude in place and replaces Codex in the same workbench column', () => {
    expect(picker).toContain("type TargetEngineId = 'claude-code' | 'codex'")
    expect(picker).toContain('promotePendingTaskToDraft(props.sessionId)')
    expect(picker).toContain('stageEngineDraft(replacementSessionId')
    expect(picker).toContain('replaceWorkbenchSession(props.sessionId, replacementSessionId)')
    expect(picker).toContain('discardStagedSession(replacementSessionId)')
  })

  it('stacks engine choices when the workbench column is too narrow', () => {
    expect(choicePanel).toContain('container-type: inline-size')
    expect(choicePanel).toContain('grid-template-columns: minmax(0, 1fr)')
    expect(choicePanel).toContain('@container (min-width: 480px)')
    expect(choicePanel).toContain('grid-template-columns: repeat(2, minmax(0, 1fr))')
    expect(choicePanel).toContain('min-width: 0')
  })
})

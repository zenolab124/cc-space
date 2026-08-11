import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('multi-engine race architecture', () => {
  it('routes standard-engine race turns through the engine runtime', () => {
    const raceInput = source('../../src/composables/useRaceInput.ts')

    expect(raceInput).toContain('startTurnWithInput(context.reference')
    expect(raceInput).toContain('attachSession(context.reference, engineRuntimeOptions(sessionId))')
    expect(raceInput).toContain('sendInputWhileRunning(')
    expect(raceInput).toContain('actions.sendWhileRunning.available')
    expect(raceInput).toContain('if (!context.runtimeDraft)')
    expect(raceInput).toContain('context.runtimeDraftChannel !== engineRuntimeChannel(sessionId)')
    expect(raceInput).toContain('const targets = race.lanes.map')
    expect(raceInput).toContain('targets.map(async')
    expect(raceInput).toContain('if (broadcasting.value || raceMutationLoading.value) return')
    expect(raceInput).toContain('forkSession(context.reference, null, engineRuntimeOptions(sourceLane.sessionId))')
    expect(raceInput).toContain('interruptTurn(')
    expect(raceInput).toContain('createSession(context.project, race.cwd')
    expect(raceInput).toContain('lockRaceEngineSelection(tab.value.id)')
  })

  it('derives workbench actions from engine capabilities', () => {
    const column = source('../../src/components/workbench/WorkbenchColumn.vue')
    const raceColumns = source('../../src/components/workbench/RaceColumns.vue')

    expect(column).toContain('resolveWorkbenchEngineActions')
    expect(column).toContain('v-if="canRace"')
    expect(column).toContain('v-if="canFork"')
    expect(column).toContain('v-if="canCreate"')
    expect(column).toContain(':disabled="mutationDisabled"')
    expect(raceColumns).toContain(':mutation-disabled="broadcasting || raceMutationLoading"')
  })

  it('allows engine changes only before the first successful race broadcast', () => {
    const workbench = source('../../src/composables/useWorkbench.ts')
    const raceInput = source('../../src/composables/useRaceInput.ts')
    const column = source('../../src/components/workbench/WorkbenchColumn.vue')
    const raceColumns = source('../../src/components/workbench/RaceColumns.vue')

    expect(workbench).toContain('engineSwitchLocked: boolean')
    expect(workbench).toContain('replaceRaceLaneSession')
    expect(workbench).toContain('engineSwitchLocked: false')
    expect(raceInput).toContain('switchLaneEngine')
    expect(raceInput).toContain('usesNativeSessionSurface(target.instance)')
    expect(raceInput).toContain('projectForEngine(target, race.cwd)')
    expect(raceInput).toContain('setEngineRunConfig(replacementSessionId')
    expect(raceInput).toContain('const hasNativeLane = targets.some')
    expect(raceInput).toContain("results.some(result => result.status === 'fulfilled')")
    expect(column).toContain('workbench-column-engine-switch')
    expect(column).toContain('raceEngineSwitchLocked')
    expect(raceColumns).toContain('@switch-race-engine="switchLaneEngine"')
  })
})

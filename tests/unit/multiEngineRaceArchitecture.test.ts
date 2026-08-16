import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('multi-engine race architecture', () => {
  it('routes standard-engine race turns through the engine runtime', () => {
    const raceInput = source('../../src/composables/useRaceInput.ts')
    const draftChannel = source('../../src/engines/draftChannel.ts')

    expect(raceInput).toContain('startTurnWithInput(context.reference')
    expect(raceInput).toContain('attachSession(context.reference, engineRuntimeOptions(sessionId))')
    expect(raceInput).toContain('sendInputWhileRunning(')
    expect(raceInput).toContain('actions.sendWhileRunning.available')
    expect(raceInput).toContain('if (!context.runtimeDraft)')
    expect(raceInput).toContain('async function rebindRuntimeDraftChannel')
    expect(raceInput).toContain('rebindDraftChannel({')
    expect(raceInput).toContain('targets = await Promise.all(targets.map(rebindRuntimeDraftChannel))')
    expect(draftChannel).toContain('attachedChannel: request.selectedChannel')
    expect(raceInput).toContain('replaceRaceLaneSession(tab.value.id, source, next)')
    expect(raceInput).toContain("tab.value.race?.engineSwitchLocked")
    expect(raceInput).toContain('let targets: LaneTarget[] = race.lanes.map')
    expect(raceInput).toContain('targets.map(async')
    expect(raceInput).toContain('if (broadcasting.value || raceMutationLoading.value) return')
    expect(raceInput).toContain('runtimeDraft: !!draft && !summary')
    expect(raceInput).toContain('await createSession(context.project, context.cwd, options)')
    expect(raceInput).toContain('await forkSession(context.reference, null, options)')
    expect(raceInput).toContain('interruptTurn(')
    expect(raceInput).toContain('createSession(context.project, race.cwd')
    expect(raceInput).toContain('lockRaceEngineSelection(tab.value.id)')
  })

  it('broadcasts command-looking input verbatim without using the command parser', () => {
    const raceInput = source('../../src/composables/useRaceInput.ts')

    expect(raceInput).not.toContain('useSlashCommands')
    expect(raceInput).not.toContain('parseCommand')
    expect(raceInput).not.toContain('slashError')
    expect(raceInput).toContain("genericInput.push({ kind: 'text', text })")
    expect(raceInput).toContain('return sendMessage(sessionId, race.cwd, text')
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
    const racePicker = source('../../src/components/workbench/RaceEnginePicker.vue')
    const choicePanel = source('../../src/components/workbench/EngineChoicePanel.vue')
    const workbenchColumns = source('../../src/components/workbench/WorkbenchColumns.vue')

    expect(workbench).toContain('engineSwitchLocked: boolean')
    expect(workbench).toContain('replaceRaceLaneSession')
    expect(workbench).toContain('engineSwitchLocked: false')
    expect(raceInput).toContain('switchLaneEngine(sessionId: string, targetEngineId: string)')
    expect(raceInput).toContain('choices.find(engine => engine.instance.engineId === targetEngineId)')
    expect(raceInput).toContain('usesNativeSessionSurface(target.instance)')
    expect(raceInput).toContain('projectForEngine(target, race.cwd)')
    expect(raceInput).toContain('setEngineRunConfig(replacementSessionId')
    expect(raceInput).toContain('const hasNativeLane = targets.some')
    expect(raceInput).toContain("results.some(result => result.status === 'fulfilled')")
    expect(column).toContain('workbench-column-engine-switch')
    expect(column).toContain('raceEngineSwitchLocked')
    expect(column).toContain('<RaceEnginePicker')
    expect(column).toContain("emit('toggleRaceEnginePicker', column.sessionId)")
    expect(raceColumns).toContain('@toggle-race-engine-picker="toggleRaceEnginePicker"')
    expect(raceColumns).toContain('@select-race-engine="selectRaceEngine"')
    expect(racePicker).toContain('<EngineChoicePanel')
    expect(choicePanel).toContain('engine-choice-grid')
    expect(workbenchColumns).toContain('const runtimeDraft = !!draft && !session')
    expect(workbenchColumns).toContain('await createSession(project, cwd, options)')
    expect(workbenchColumns).toContain('await forkSession(reference, null, options)')
  })

  it('reveals and focuses a newly added race lane', () => {
    const raceInput = source('../../src/composables/useRaceInput.ts')
    const raceColumns = source('../../src/components/workbench/RaceColumns.vue')

    expect(raceInput).toContain('async function forkNewLane(): Promise<string | null>')
    expect(raceInput).toContain('return sessionId')
    expect(raceInput).toContain('return newSessionId')
    expect(raceColumns).toContain('const sessionId = await forkNewLane()')
    expect(raceColumns).toContain('enginePickerSessionId.value = sessionId')
    expect(raceColumns).toContain(':data-workbench-session-id="col.sessionId"')
    expect(raceColumns).toContain('scrollLaneIntoView(sessionId)')
    expect(raceColumns).toContain("scrollIntoView({ behavior: 'smooth', inline: 'nearest', block: 'nearest' })")
  })
})

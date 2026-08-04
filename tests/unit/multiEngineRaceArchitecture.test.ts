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
    expect(raceInput).toContain('forkSession(context.reference)')
    expect(raceInput).toContain('interruptTurn(')
    expect(raceInput).toContain('createSession(context.project!')
  })

  it('derives workbench actions from engine capabilities', () => {
    const column = source('../../src/components/workbench/WorkbenchColumn.vue')

    expect(column).toContain('resolveWorkbenchEngineActions')
    expect(column).toContain('v-if="canRace"')
    expect(column).toContain('v-if="canFork"')
    expect(column).toContain('v-if="canCreate"')
  })
})

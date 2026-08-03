import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const source = readFileSync(
  fileURLToPath(new URL('../../src/components/SessionDetail.vue', import.meta.url)),
  'utf8',
)

describe('SessionDetail setup order', () => {
  it('initializes external process state before the async task ledger is watched', () => {
    const externalState = source.indexOf('const externalRunning = ref(false)')
    const taskLedger = source.indexOf('const asyncTasks = computed<AsyncTaskItem[]>')
    const taskWatch = source.indexOf('watch(asyncTasks,')

    expect(externalState).toBeGreaterThanOrEqual(0)
    expect(taskLedger).toBeGreaterThan(externalState)
    expect(taskWatch).toBeGreaterThan(taskLedger)
  })
})

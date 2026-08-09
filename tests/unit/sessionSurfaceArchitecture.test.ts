import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('unified session surface architecture', () => {
  it('keeps page-level entry points independent of engine controllers', () => {
    const workbench = source('../../src/components/workbench/WorkbenchColumn.vue')
    const archive = source('../../src/views/SessionsView.vue')
    for (const entry of [workbench, archive]) {
      expect(entry).toContain('UnifiedSessionDetail')
      expect(entry).not.toContain('<EngineSessionDetail')
      expect(entry).not.toContain('<SessionDetail')
    }
  })

  it('renders native and standard records through the same visual shells', () => {
    const nativeController = source('../../src/components/SessionDetail.vue')
    const standardController = source('../../src/components/engine/EngineSessionDetail.vue')
    const nativeToolbar = source('../../src/components/topbar/SessionTopBar.vue')
    const nativeTurns = source('../../src/components/MessageGroup.vue')
    const standardTurns = source('../../src/components/engine/EngineConversationGroup.vue')
    const nativeTools = source('../../src/components/ToolProcessGroup.vue')
    const standardTools = source('../../src/components/engine/EngineSegmentBlock.vue')
    const sharedRunConfig = source('../../src/components/topbar/RunConfigCapsule.vue')
    const sharedComposer = source('../../src/components/session/SessionComposer.vue')
    const raceController = source('../../src/components/workbench/RaceColumns.vue')
    const raceInput = source('../../src/composables/useRaceInput.ts')
    const engineCenter = source('../../src/components/settings/EngineCenter.vue')
    const zhLocale = source('../../src/locales/zh-CN.json')
    const enLocale = source('../../src/locales/en-US.json')
    const engineTypes = source('../../src/engines/types.ts')
    const engineClient = source('../../src/engines/client.ts')
    const coreRuntime = source('../../src-tauri/src/engines/core/runtime.rs')
    const coordinator = source('../../src-tauri/src/engines/core/coordinator.rs')
    const engineCommands = source('../../src-tauri/src/engines/commands.rs')
    const tauriEntry = source('../../src-tauri/src/lib.rs')

    expect(nativeController).toContain('<SessionSurface')
    expect(standardController).toContain('<SessionSurface')
    expect(nativeController).toContain('<SessionIdentityBar')
    expect(standardController).toContain('<SessionIdentityBar')
    expect(nativeToolbar).toContain('<SessionToolbar')
    expect(standardController).toContain('<SessionToolbar')
    expect(nativeToolbar).toContain('<RunConfigCapsule')
    expect(standardController).toContain('<RunConfigCapsule')
    expect(nativeController).toContain('<SessionComposer')
    expect(standardController).toContain('<SessionComposer')
    expect(nativeController).toContain('can-send-while-busy')
    expect(standardController).toContain(':can-send-while-busy="canSendWhileBusy"')
    expect(sharedComposer).toContain('resolveComposerAction')
    expect(nativeController).toContain('<SessionComposerField')
    expect(standardController).toContain('<SessionComposerField')
    expect(nativeController).toContain('<SessionViewport')
    expect(standardController).toContain('<SessionViewport')
    expect(nativeController).toContain('<SessionContentState')
    expect(standardController).toContain('<SessionContentState')
    expect(nativeController).toContain('<SessionInteractionPanel')
    expect(standardController).toContain('<SessionInteractionPanel')
    expect(nativeController).toContain('<SessionReadonlyBar')
    expect(standardController).toContain('<SessionReadonlyBar')
    expect(nativeTurns).toContain('<ConversationTurn')
    expect(standardTurns).toContain('<ConversationTurn')
    expect(nativeTools).toContain('<SessionProcessDisclosure')
    expect(standardTools).toContain('<SessionProcessDisclosure')
    expect(sharedRunConfig).not.toContain("engine: 'Codex'")
    expect(sharedRunConfig).not.toContain('activeEngineChannel.value?.codex')
    expect(standardController).not.toContain('?.codex')
    expect(standardController).toContain('engineChannelBinding')
    expect(engineTypes).toContain('sendWhileRunning')
    expect(engineClient).toContain('sendInputWhileRunning')
    expect(standardController).toContain('if (sendWhileRunning && !canSendWhileBusy.value)')
    expect(standardController).toContain('if (!isBusy.value && !(await ensureAttached())) return')
    expect(standardController).toContain('if (isBusy.value) return runtimeId.value !== null')
    expect(standardController).toContain('void send()')
    expect(coreRuntime).toContain('send_input_while_running')
    expect(coordinator).toContain('send_input_while_running')
    expect(engineCommands).toContain('engine_send_input_while_running')
    expect(tauriEntry).toContain('engine_send_input_while_running')
    for (const genericSurface of [
      engineTypes,
      engineClient,
      standardController,
      sharedComposer,
      raceController,
      raceInput,
      engineCenter,
      zhLocale,
      enLocale,
      coreRuntime,
      coordinator,
      engineCommands,
      tauriEntry,
    ]) {
      expect(genericSurface).not.toMatch(/steer|追加指令/i)
    }
  })
})

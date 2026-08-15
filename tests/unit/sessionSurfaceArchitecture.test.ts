import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('unified session surface architecture', () => {
  it('places the shared fast-mode checkbox above effort for Claude and supported Codex models', () => {
    const capsule = source('../../src/components/topbar/RunConfigCapsule.vue')
    const standardController = source('../../src/components/engine/EngineSessionDetail.vue')
    const fastModePosition = capsule.indexOf("$t('topbar.fastMode')")
    const effortPosition = capsule.indexOf("$t('topbar.effortLabel')")

    expect(fastModePosition).toBeGreaterThan(-1)
    expect(fastModePosition).toBeLessThan(effortPosition)
    expect(capsule).toContain('v-if="showFastMode"')
    expect(capsule).toContain('props.engineConfig?.showFastMode === true')
    expect(standardController).toContain("showFastMode: sessionEngineId.value === 'codex'")
    expect(capsule).toContain('type="checkbox"')
  })

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
    const sharedBlocks = source('../../src/components/ContentBlockList.vue')
    const toolDisplaySettings = source('../../src/composables/useToolDisplay.ts')
    const settingsView = source('../../src/views/SettingsView.vue')
    const sharedBashCard = source('../../src/components/blocks/tools/ToolBash.vue')
    const sharedRunConfig = source('../../src/components/topbar/RunConfigCapsule.vue')
    const sharedComposer = source('../../src/components/session/SessionComposer.vue')
    const sharedAttachments = source('../../src/components/session/SessionComposerAttachments.vue')
    const sharedQueue = source('../../src/components/session/SessionComposerQueue.vue')
    const sharedApproval = source('../../src/components/session/SessionApprovalCard.vue')
    const sharedSidePanel = source('../../src/components/session/SessionSidePanel.vue')
    const sharedSidePanelHost = source('../../src/composables/useSessionSidePanelHost.ts')
    const nativeApproval = source('../../src/components/PermissionCard.vue')
    const sharedTyping = source('../../src/components/session/SessionTypingIndicator.vue')
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
    expect(sharedComposer).toContain('border-accent bg-accent text-white hover:text-white')
    expect(sharedComposer).not.toContain('bg-accent text-accent-foreground')
    expect(nativeController).toContain('<SessionComposerField')
    expect(standardController).toContain('<SessionComposerField')
    expect(nativeController).toContain('<SessionComposerAttachments')
    expect(standardController).toContain('<SessionComposerAttachments')
    expect(nativeController).toContain('<SessionComposerQueue')
    expect(standardController).toContain('<SessionComposerQueue')
    expect(sharedAttachments).toContain('PendingImage')
    expect(sharedQueue).toContain('ComposerQueueItem')
    expect(nativeController).toContain('<SessionViewport')
    expect(standardController).toContain('<SessionViewport')
    expect(nativeController).toContain('<SessionTypingIndicator')
    expect(standardController).toContain('<SessionTypingIndicator')
    expect(sharedTyping).toContain('absolute bottom-0')
    expect(standardController).toContain('ref="timelineContentElement" class="pb-2 relative"')
    const standardTimeline = standardController.slice(
      standardController.indexOf('ref="timelineContentElement"'),
      standardController.indexOf('<SessionContentState v-if="error"'),
    )
    expect(standardTimeline).toContain('<SessionTypingIndicator')
    expect(nativeController).not.toContain(':hide-user=')
    expect(standardController).not.toContain(':hide-user=')
    expect(sharedRunConfig).not.toContain('engineConfig?.inheritedChannelLabel')
    expect(sharedRunConfig).toContain("t(props.engineConfig || props.defaultConfig ? 'topbar.channelFollowEngine' : 'topbar.channelOfficial'")
    const nativeActiveTail = nativeController.slice(
      nativeController.indexOf('v-if="lastGroup"'),
      nativeController.indexOf('<!-- 锚点失效的切换横线兜底'),
    )
    expect(nativeActiveTail).not.toContain('sticky-user')
    expect(sharedTyping).toContain('session-typing-dot')
    expect(nativeController).toContain('<SessionContentState')
    expect(standardController).toContain('<SessionContentState')
    expect(nativeController).toContain('<SessionInteractionPanel')
    expect(standardController).toContain('<SessionInteractionPanel')
    expect(nativeApproval).toContain('<SessionApprovalCard')
    expect(standardController).toContain('<SessionApprovalCard')
    expect(sharedApproval).toContain('<SessionInteractionCard')
    expect(nativeController).toContain('<SessionSidePanel')
    expect(standardController).toContain('<SessionSidePanel')
    expect(nativeController).toContain('useSessionSidePanelHost(sidePanelVisible')
    expect(standardController).toContain('useSessionSidePanelHost(asyncPanelVisible')
    expect(sharedSidePanel).toContain('session-side-panel')
    expect(sharedSidePanelHost).toContain('context.tab.columnSizes[context.index] = targetWidth.value * 2')
    expect(nativeController).toContain('<SessionReadonlyBar')
    expect(standardController).toContain('<SessionReadonlyBar')
    expect(nativeTurns).toContain('<ConversationTurn')
    expect(standardTurns).toContain('<ConversationTurn')
    expect(standardTurns).toContain('<ContentBlockList')
    expect(standardTurns).toContain('projectEngineProcessEntries')
    expect(standardTurns).not.toContain('EngineProcessGroup')
    expect(sharedBlocks).toContain("toolDisplayMode === 'cards'")
    expect(sharedBlocks).toContain("toolDisplayMode === 'individual'")
    expect(sharedBlocks).toContain('props.displayMode ?? defaultToolDisplayMode.value')
    expect(nativeTurns).toContain("toolDisplayModeFor('claude-code', record.message?.model")
    expect(standardTurns).toContain('toolDisplayModeFor(')
    expect(standardTurns).toContain(':display-mode="block.displayMode"')
    expect(toolDisplaySettings).toContain("const OVERRIDES_SETTING_KEY = 'toolDisplayModeOverrides'")
    expect(toolDisplaySettings).toContain('toolDisplayModeOverrides.value[key]')
    expect(settingsView).toContain('v-model="selectedToolDisplayTargetKey"')
    expect(settingsView).toContain('@click="resetSelectedToolDisplayMode"')
    expect(sharedBashCard).toContain("expanded ? '' : 'line-clamp-3'")
    expect(standardController).toContain('provideToolFoldState()')
    expect(standardController).toContain(':show-thought-process="enginePresentation.showThoughtProcess"')
    expect(standardTurns).toContain('isRenderableEngineSegment(segment, props.showThoughtProcess !== false)')
    expect(standardTurns).toContain('record.sourceMeta.optimisticImages')
    expect(standardTurns).toContain('record.sourceMeta.turnError === true')
    expect(standardTurns).toContain('<SessionContentState v-if="turnError" tone="danger">')
    expect(nativeTools).toContain('<SessionProcessDisclosure')
    expect(standardTools).toContain('<MessageBlock')
    expect(standardTools).not.toContain('<SessionProcessDisclosure')
    expect(sharedRunConfig).not.toContain("engine: 'Codex'")
    expect(sharedRunConfig).not.toContain('activeEngineChannel.value?.codex')
    expect(standardController).not.toContain('?.codex')
    expect(standardController).toContain('engineChannelBinding')
    expect(engineTypes).toContain('sendWhileRunning')
    expect(engineClient).toContain('sendInputWhileRunning')
    expect(standardController).toContain('queuedInputs.value.push(queuedItem)')
    expect(standardController).toContain("event.payload.phase === 'idle' && visualActiveTurnId.value === null")
    expect(standardController).toContain('startTurnWithFastFallback(reference.value, item.input)')
    expect(standardController).toContain('startTurnWithInput(session, inputItems, selectedTurnOptions(requestedServiceTier))')
    expect(standardController).not.toContain('sendInputWhileRunning(')
    expect(standardController).toContain("if (isBusy.value) return runtimeId.value !== null ? 'attached' : 'failed'")
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

  it('defers Codex attachment until send and forks only after an explicit conflict choice', () => {
    const controller = source('../../src/components/engine/EngineSessionDetail.vue')
    const mounted = controller.slice(
      controller.indexOf('onMounted(async () =>'),
      controller.indexOf('onUnmounted(() =>'),
    )

    expect(mounted).toContain('loadRuntimeConfiguration()')
    expect(mounted).not.toContain('ensureAttached()')
    expect(controller).toContain("type AttachOutcome = 'attached' | 'writer-conflict' | 'failed'")
    expect(controller).toContain("const choice = await confirmMulti(t('engine.codex.writerConflictMessage')")
    expect(controller).toContain("if (choice === 'retry') continue")
    expect(controller).toContain("if (choice === 'fork') return await forkAndSend(inputItems)")
  })

  it('rebinds a blank standard session when its runtime configuration changes before the first turn', () => {
    const controller = source('../../src/components/engine/EngineSessionDetail.vue')
    const draftChannel = source('../../src/engines/draftChannel.ts')

    expect(controller).toContain('async function rebindCurrentDraftChannel()')
    expect(controller).toContain('draft.attachedCapabilityFingerprint !== capabilityFingerprint')
    expect(controller).toContain('!sameRuntimeChannel(draft.attachedChannel, selectedChannel.value)')
    expect(controller).toContain('beforeReplace: value => { pendingDraftReplacement = value }')
    expect(controller).toContain('replaceSession: replaceWorkbenchSession')
    expect(controller).not.toContain("error.value = t('engine.draftChannelLocked')")
    expect(draftChannel).toContain('dependencies.discardDraft(replacementSessionId)')
  })

  it('shows the shared session context banner when a standard runtime is created or resumed', () => {
    const nativeController = source('../../src/components/SessionDetail.vue')
    const standardController = source('../../src/components/engine/EngineSessionDetail.vue')
    const sharedBanner = source('../../src/components/session/SessionBannerOverlay.vue')

    expect(nativeController).toContain("import SessionBannerOverlay from './session/SessionBannerOverlay.vue'")
    expect(standardController).toContain("import SessionBannerOverlay from '@/components/session/SessionBannerOverlay.vue'")
    expect(nativeController).toContain('<SessionBannerOverlay')
    expect(standardController).toContain('<SessionBannerOverlay')
    expect(sharedBanner).toContain("import SessionBanner from '@/components/SessionBanner.vue'")
    expect(sharedBanner).toContain('<SessionBanner')
    expect(standardController).toContain('announceCurrentRuntime()')
    expect(standardController).toContain('showSessionBanner(!engineDraft(props.session.id))')
    expect(standardController).toContain('<template #overlay>')
  })

  it('reconciles normalized runtime events without fixed-delay live record clearing', () => {
    const controller = source('../../src/components/engine/EngineSessionDetail.vue')
    const runtimeTypes = source('../../src/engines/types.ts')

    expect(runtimeTypes).toContain('export type NormalizedRuntimeEvent =')
    expect(runtimeTypes).toContain("| { kind: 'turnStarted'; turnId: string }")
    expect(controller).toContain('reduceRuntimeTimeline')
    expect(controller).toContain('reconcileLiveRecords')
    expect(controller).toContain('scheduleTurnSettlement')
    expect(controller).toContain('useRuntimeDeltaShaper')
    expect(controller).toContain('runtimeDeltaShaper.push(envelope)')
    expect(controller).toContain('reduceRuntimeVisualActivity')
    expect(controller).not.toContain('if (event.payload.lastError) error.value = event.payload.lastError')
    expect(controller).toContain(':streaming="isTurnStreaming(group.turnId)"')
    expect(controller).not.toContain('liveRecords.value = []\n    }, 80)')
  })

  it('routes standard assistant text through the shared incremental markdown renderer', () => {
    const turns = source('../../src/components/engine/EngineConversationGroup.vue')
    const segment = source('../../src/components/engine/EngineSegmentBlock.vue')

    expect(turns).toContain(':streaming="streaming"')
    expect(turns).toContain('showFooter: props.streaming !== true')
    expect(segment).toContain('<MessageBlock :block="contentBlock" :streaming="streaming"')
  })

  it('virtualizes standard history while keeping the active tail directly rendered', () => {
    const controller = source('../../src/components/engine/EngineSessionDetail.vue')
    const asyncPanel = source('../../src/components/engine/EngineAsyncTaskPanel.vue')

    expect(controller).toContain('const conversationVirtualizer = useVirtualizer')
    expect(controller).toContain('const historicalGroups = computed')
    expect(controller).toContain('const lastConversationGroup = computed')
    expect(controller).toContain('conversationVirtualizer.getVirtualItems()')
    expect(controller).toContain('v-if="lastConversationGroup"')
    expect(controller).toContain('class="engine-sticky-user-overlay"')
    expect(controller).not.toContain(':hide-user=')
    expect(controller).toContain(':data-conversation-index="conversationGroups.length - 1"')
    const virtualHistory = controller.slice(
      controller.indexOf('v-for="virtualItem in conversationVirtualizer.getVirtualItems()"'),
      controller.indexOf('<div v-else class="space-y-4">'),
    )
    expect(virtualHistory).toContain('sticky-source-hidden')
    const activeTail = controller.slice(
      controller.indexOf('v-if="lastConversationGroup"'),
      controller.indexOf('<SessionTypingIndicator'),
    )
    expect(activeTail).toContain('sticky-source-hidden')
    expect(controller).toContain('nextCard.getBoundingClientRect().top - STICKY_CARD_GAP - restingBottom')
    expect(controller).toContain('groupConversationRecords(allRecords.value)')
    expect(asyncPanel).toContain('groupConversationRecords(records.value)')
  })
})

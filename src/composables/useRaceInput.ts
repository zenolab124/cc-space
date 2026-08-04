import { ref, computed, type Ref } from 'vue'
import i18n from '../locales'
import type { WorkbenchTab } from './useWorkbench'
import { useWorkbench } from './useWorkbench'
import { inheritRunSettings } from './useSessionSettings'
import { useStreaming, getStream } from './useStreaming'
import { useImageInput } from './useImageInput'
import { refreshChannels, useChannels } from './useChannels'
import { refreshCliDefaults, readCliDefaults } from './useCliDefaults'
import { resolveRunConfig } from './useRunConfig'
import { getSessionSettings } from './useSessionSettings'
import { parseCommand } from './useSlashCommands'
import { createSession, forkSession, interruptTurn, startTurnWithInput } from '@/engines/client'
import { resolveSession } from '@/engines/directory'
import { sessionUiId, usesNativeSessionSurface } from '@/engines/integration'
import { engineRuntimeSnapshot } from '@/engines/runtimeState'
import { engineRunConfig, inheritEngineRunConfig } from '@/engines/runConfig'
import type { ProjectRef, RuntimeInputItem, SessionRef } from '@/engines/types'

export function useRaceInput(tab: Ref<WorkbenchTab>) {
  const inputText = ref('')
  const textareaRef = ref<HTMLTextAreaElement>()
  // 拖拽收图区由组件侧绑定(整个赛马区,拖到任意位置都进共享输入)
  const dropAreaRef = ref<HTMLElement>()
  const imageInput = useImageInput({ pasteTarget: textareaRef, dropTarget: dropAreaRef })
  const slashError = ref<string | null>(null)

  const { sendMessage, stopStreaming } = useStreaming()
  const {
    addRaceLane,
    engineDraft,
    forkSourceOf,
    resetRaceLanes,
    stageEngineDraft,
  } = useWorkbench()
  const { channels, defaultSessionChannel } = useChannels()
  const raceError = ref<string | null>(null)
  const raceMutationLoading = ref(false)

  interface LaneContext {
    reference: SessionRef | null
    project: ProjectRef | null
    engineName: string
    cwd: string
    native: boolean
  }

  function laneContext(sessionId: string): LaneContext {
    const summary = resolveSession(sessionId)
    const draft = engineDraft(sessionId)
    const reference = summary?.reference ?? draft?.reference ?? null
    return {
      reference,
      project: summary?.project_reference ?? draft?.project ?? null,
      engineName: summary?.engine_name ?? draft?.engineName ?? reference?.engine.engineId ?? 'Agent',
      cwd: summary?.cwd ?? draft?.cwd ?? tab.value.race?.cwd ?? '',
      native: !reference || usesNativeSessionSurface(reference.engine),
    }
  }

  function laneRunning(sessionId: string): boolean {
    const context = laneContext(sessionId)
    if (context.native) return getStream(sessionId).streaming
    const snapshot = engineRuntimeSnapshot(sessionId)
    return snapshot?.phase === 'running' || snapshot?.phase === 'awaitingInteraction'
  }

  const anyStreaming = computed(() => {
    const race = tab.value.race
    if (!race) return false
    return race.lanes.some(lane => laneRunning(lane.sessionId))
  })

  const streamingCount = computed(() => {
    const race = tab.value.race
    if (!race) return 0
    return race.lanes.filter(lane => laneRunning(lane.sessionId)).length
  })

  async function broadcastSend() {
    const race = tab.value.race
    if (!race) return
    const text = inputText.value.trim()
    if (!text && !imageInput.images.value.length) return

    const contexts = race.lanes.map(lane => laneContext(lane.sessionId))
    const nativeRace = contexts.every(context => context.native)
    if (nativeRace) {
      const parsed = parseCommand(text)
      if (parsed.kind === 'invalid') {
        slashError.value = parsed.reason
        return
      }
      if (parsed.kind === 'native' || parsed.kind === 'terminal') return
    }
    slashError.value = null
    raceError.value = null

    inputText.value = ''
    if (textareaRef.value) textareaRef.value.style.height = 'auto'

    const images = imageInput.images.value.length ? await imageInput.toImageBlocks() : undefined
    const genericInput: RuntimeInputItem[] = []
    if (text) genericInput.push({ kind: 'text', text })
    for (const image of images ?? []) {
      genericInput.push({
        kind: 'image',
        mediaType: image.source.media_type,
        data: image.source.data,
      })
    }
    imageInput.clearImages()
    if (nativeRace) await Promise.all([refreshChannels(), refreshCliDefaults(race.cwd)])
    const snapshot = {
      channels: channels.value,
      defaultSessionChannel: defaultSessionChannel.value,
      cliSettings: readCliDefaults(race.cwd),
    }

    const promises = race.lanes.map((lane, index) => {
      const context = contexts[index]
      if (!context.native && context.reference) {
        const config = engineRunConfig(lane.sessionId)
        return startTurnWithInput(context.reference, genericInput, {
          cwd: context.cwd,
          ...(config?.model ? { model: config.model } : {}),
          ...(config?.effort ? { effort: config.effort } : {}),
        })
      }
      const settings = getSessionSettings(lane.sessionId)
      const rc = resolveRunConfig(settings, snapshot)
      return sendMessage(lane.sessionId, race.cwd, text, {
        model: rc.launch.model,
        effort: rc.launch.effort ?? null,
        channel: rc.channelId,
        advisor: settings.advisor,
        chrome: settings.chrome,
        forkSource: forkSourceOf(lane.sessionId) ?? undefined,
        extraArgs: settings.extraArgs || undefined,
        images,
        permissionMode: rc.launch.permissionMode ?? undefined,
      })
    })
    const results = await Promise.allSettled(promises)
    const failures = results.filter(result => result.status === 'rejected')
    if (failures.length) raceError.value = String((failures[0] as PromiseRejectedResult).reason)
  }

  function stopAll() {
    const race = tab.value.race
    if (!race) return
    for (const lane of race.lanes) {
      const context = laneContext(lane.sessionId)
      if (context.native && getStream(lane.sessionId).streaming) {
        stopStreaming(lane.sessionId)
      } else if (!context.native && context.reference) {
        const snapshot = engineRuntimeSnapshot(lane.sessionId)
        if (snapshot?.activeTurnId) {
          void interruptTurn(
            context.reference,
            snapshot.runtimeId,
            snapshot.activeTurnId,
          ).catch(error => { raceError.value = String(error) })
        }
      }
    }
  }

  async function forkNewLane() {
    if (raceMutationLoading.value) return
    const race = tab.value.race
    if (!race || race.lanes.length === 0) return
    const sourceLane = race.lanes[0]
    const context = laneContext(sourceLane.sessionId)
    raceError.value = null
    raceMutationLoading.value = true
    try {
      if (!context.native && context.reference && context.project) {
        const created = await forkSession(context.reference)
        const sessionId = sessionUiId(created.session)
        stageEngineDraft(sessionId, {
          reference: created.session,
          project: context.project,
          engineName: context.engineName,
          cwd: context.cwd,
        })
        inheritEngineRunConfig(sourceLane.sessionId, sessionId)
        addRaceLane(tab.value.id, sessionId)
        return
      }
      const { registerFork } = useWorkbench()
      const newSessionId = crypto.randomUUID()
      // 无条件登记分叉意图:源有无历史由 Rust 端按源 jsonl 真值判决(未落盘则退化新建)
      registerFork(newSessionId, sourceLane.sessionId, race.cwd)
      inheritRunSettings(sourceLane.sessionId, newSessionId)
      addRaceLane(tab.value.id, newSessionId)
    } catch (error) {
      raceError.value = String(error)
    } finally {
      raceMutationLoading.value = false
    }
  }

  async function resetAllLanes() {
    if (raceMutationLoading.value) return
    const race = tab.value.race
    if (!race || race.lanes.length === 0) return
    const context = laneContext(race.lanes[0].sessionId)
    raceError.value = null
    if (context.native) {
      resetRaceLanes(tab.value.id)
      return
    }
    if (!context.project) {
      raceError.value = i18n.global.t('common.runtimeUnavailable')
      return
    }
    raceMutationLoading.value = true
    try {
      const created = await Promise.all(
        race.lanes.map(() => createSession(context.project!, race.cwd)),
      )
      const ids = created.map((runtime, index) => {
        const sourceSessionId = race.lanes[index].sessionId
        const sessionId = sessionUiId(runtime.session)
        stageEngineDraft(sessionId, {
          reference: runtime.session,
          project: context.project!,
          engineName: context.engineName,
          cwd: race.cwd,
        })
        inheritEngineRunConfig(sourceSessionId, sessionId)
        return sessionId
      })
      resetRaceLanes(tab.value.id, ids)
    } catch (error) {
      raceError.value = String(error)
    } finally {
      raceMutationLoading.value = false
    }
  }

  return {
    inputText,
    textareaRef,
    dropAreaRef,
    imageInput,
    slashError,
    raceError,
    raceMutationLoading,
    anyStreaming,
    streamingCount,
    broadcastSend,
    stopAll,
    forkNewLane,
    resetAllLanes,
  }
}

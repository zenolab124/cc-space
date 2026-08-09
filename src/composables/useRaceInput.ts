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
import {
  attachSession,
  createSession,
  forkSession,
  interruptTurn,
  sendInputWhileRunning,
  sessionActions,
  startTurnWithInput,
} from '@/engines/client'
import { resolveSession } from '@/engines/directory'
import { sessionUiId, usesNativeSessionSurface } from '@/engines/integration'
import { engineRuntimeSnapshot } from '@/engines/runtimeState'
import { engineRunConfig, engineRuntimeChannel, engineRuntimeOptions, inheritEngineRunConfig } from '@/engines/runConfig'
import type { ProjectRef, RuntimeInputItem, RuntimeSnapshot, SessionRef } from '@/engines/types'

function errorMessage(cause: unknown): string {
  if (typeof cause === 'string') return cause
  if (cause && typeof cause === 'object' && 'message' in cause) {
    const message = (cause as { message?: unknown }).message
    if (typeof message === 'string') return message
  }
  return String(cause)
}

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
  const broadcasting = ref(false)

  interface LaneContext {
    reference: SessionRef | null
    project: ProjectRef | null
    engineName: string
    cwd: string
    native: boolean
    runtimeDraft: boolean
    runtimeDraftChannel: string | null | undefined
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
      runtimeDraft: !!draft,
      runtimeDraftChannel: draft?.attachedChannel,
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
    if (broadcasting.value || raceMutationLoading.value) return
    const race = tab.value.race
    if (!race) return
    const text = inputText.value.trim()
    if (!text && !imageInput.images.value.length) return

    const targets = race.lanes.map(lane => ({
      sessionId: lane.sessionId,
      context: laneContext(lane.sessionId),
    }))
    const nativeRace = targets.every(target => target.context.native)
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
    broadcasting.value = true
    try {
      const pendingImages = [...imageInput.images.value]
      const images = pendingImages.length ? await imageInput.toImageBlocks(pendingImages) : undefined
      const genericInput: RuntimeInputItem[] = []
      if (text) genericInput.push({ kind: 'text', text })
      for (const image of images ?? []) {
        genericInput.push({
          kind: 'image',
          mediaType: image.source.media_type,
          data: image.source.data,
        })
      }

      // 先验证所有运行中的标准 lane，避免部分投递后才发现某个 adapter
      // 不支持运行中输入。原生 Claude lane 仍由 useStreaming 排到下一轮。
      const runningStandardSnapshots = new Map<string, RuntimeSnapshot>()
      await Promise.all(targets.map(async ({ sessionId, context }) => {
        if (context.native || !context.reference) return
        if (context.runtimeDraft && context.runtimeDraftChannel !== engineRuntimeChannel(sessionId)) {
          throw new Error(i18n.global.t('engine.draftChannelLocked'))
        }
        const runtime = engineRuntimeSnapshot(sessionId)
        const running = runtime?.phase === 'running' || runtime?.phase === 'awaitingInteraction'
        if (!running) return
        if (!runtime.activeTurnId) throw new Error(i18n.global.t('common.runtimeUnavailable'))
        const actions = await sessionActions(context.reference)
        if (!actions.sendWhileRunning.available) {
          const reason = actions.sendWhileRunning.reasonCode
          throw new Error(reason
            ? i18n.global.t(reason, i18n.global.t('common.runtimeUnavailable'))
            : i18n.global.t('common.runtimeUnavailable'))
        }
        runningStandardSnapshots.set(sessionId, runtime)
      }))

      // 广播前的异步准备期间若目标集合被外部状态改写，整批终止，避免错配。
      if (targets.some(target => !race.lanes.some(lane => lane.sessionId === target.sessionId))) {
        throw new Error(i18n.global.t('common.runtimeUnavailable'))
      }

      if (nativeRace) await Promise.all([refreshChannels(), refreshCliDefaults(race.cwd)])
      const snapshot = {
        channels: channels.value,
        defaultSessionChannel: defaultSessionChannel.value,
        cliSettings: readCliDefaults(race.cwd),
      }
      inputText.value = ''
      if (textareaRef.value) textareaRef.value.style.height = 'auto'
      for (const image of pendingImages) imageInput.removeImage(image.id)
      imageInput.clearError()

      const promises = targets.map(async ({ sessionId, context }) => {
        if (!context.native && context.reference) {
          const running = runningStandardSnapshots.get(sessionId)
          if (running?.activeTurnId) {
            return sendInputWhileRunning(
              context.reference,
              running.runtimeId,
              running.activeTurnId,
              genericInput,
            )
          }
          const config = engineRunConfig(sessionId)
          if (!context.runtimeDraft) {
            await attachSession(context.reference, engineRuntimeOptions(sessionId))
          }
          return startTurnWithInput(context.reference, genericInput, {
            cwd: context.cwd,
            ...(config?.model ? { model: config.model } : {}),
            ...(config?.effort ? { effort: config.effort } : {}),
          })
        }
        const settings = getSessionSettings(sessionId)
        const rc = resolveRunConfig(settings, snapshot)
        return sendMessage(sessionId, race.cwd, text, {
          model: rc.launch.model,
          effort: rc.launch.effort ?? null,
          channel: rc.channelId,
          advisor: settings.advisor,
          chrome: settings.chrome,
          forkSource: forkSourceOf(sessionId) ?? undefined,
          extraArgs: settings.extraArgs || undefined,
          images,
          permissionMode: rc.launch.permissionMode ?? undefined,
        })
      })
      const results = await Promise.allSettled(promises)
      const failures = results.filter(result => result.status === 'rejected')
      if (results.length > 0 && failures.length === results.length) {
        inputText.value = text
        imageInput.images.value = [...pendingImages, ...imageInput.images.value]
      }
      if (failures.length) {
        raceError.value = errorMessage((failures[0] as PromiseRejectedResult).reason)
      }
    } catch (cause) {
      raceError.value = errorMessage(cause)
    } finally {
      broadcasting.value = false
    }
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
    if (raceMutationLoading.value || broadcasting.value) return
    const race = tab.value.race
    if (!race || race.lanes.length === 0) return
    const sourceLane = race.lanes[0]
    const context = laneContext(sourceLane.sessionId)
    raceError.value = null
    raceMutationLoading.value = true
    try {
      if (!context.native && context.reference && context.project) {
        const attachedChannel = engineRuntimeChannel(sourceLane.sessionId)
        const created = await forkSession(context.reference, null, engineRuntimeOptions(sourceLane.sessionId))
        const sessionId = sessionUiId(created.session)
        stageEngineDraft(sessionId, {
          reference: created.session,
          project: context.project,
          engineName: context.engineName,
          cwd: context.cwd,
          attachedChannel,
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
    if (raceMutationLoading.value || broadcasting.value) return
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
        race.lanes.map(lane => createSession(context.project!, race.cwd, engineRuntimeOptions(lane.sessionId))),
      )
      const ids = created.map((runtime, index) => {
        const sourceSessionId = race.lanes[index].sessionId
        const sessionId = sessionUiId(runtime.session)
        stageEngineDraft(sessionId, {
          reference: runtime.session,
          project: context.project!,
          engineName: context.engineName,
          cwd: race.cwd,
          attachedChannel: engineRuntimeChannel(sourceSessionId),
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
    broadcasting,
    anyStreaming,
    streamingCount,
    broadcastSend,
    stopAll,
    forkNewLane,
    resetAllLanes,
  }
}

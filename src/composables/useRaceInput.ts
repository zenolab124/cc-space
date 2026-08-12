import { ref, computed, type Ref } from 'vue'
import i18n from '../locales'
import type { WorkbenchTab } from './useWorkbench'
import { useWorkbench } from './useWorkbench'
import { inheritRunSettings } from './useSessionSettings'
import { useStreaming, getStream } from './useStreaming'
import { useImageInput } from './useImageInput'
import { channelSupportsEngine, refreshChannels, useChannels } from './useChannels'
import { useProjects } from './useProjects'
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
import { instanceKey, sameInstance } from '@/engines/identity'
import { engineRuntimeSnapshot } from '@/engines/runtimeState'
import { engineRunConfig, engineRuntimeChannel, engineRuntimeOptions, inheritEngineRunConfig, setEngineRunConfig } from '@/engines/runConfig'
import { rebindDraftChannel, sameRuntimeChannel } from '@/engines/draftChannel'
import { useEngines } from '@/engines/useEngines'
import type { EngineDescriptor, ProjectRef, RuntimeInputItem, RuntimeSnapshot, SessionRef } from '@/engines/types'
import type { Project } from '@/types'
import { collectSessionCapabilityFingerprint } from '@/features'

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
    discardStagedSession,
    engineDraft,
    forkSourceOf,
    lockRaceEngineSelection,
    replaceRaceLaneSession,
    resetRaceLanes,
    stageDraftSession,
    stageEngineDraft,
  } = useWorkbench()
  const { projects } = useProjects()
  const { engines, health } = useEngines()
  const { channels, defaultSessionChannel, defaultSessionChannels } = useChannels()
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

  interface LaneTarget {
    sessionId: string
    context: LaneContext
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
      runtimeDraft: !!draft && !summary,
      runtimeDraftChannel: draft?.attachedChannel,
    }
  }

  function laneRunning(sessionId: string): boolean {
    const context = laneContext(sessionId)
    if (context.native) return getStream(sessionId).streaming
    const snapshot = engineRuntimeSnapshot(sessionId)
    return snapshot?.phase === 'running' || snapshot?.phase === 'awaitingInteraction'
  }

  async function rebindRuntimeDraftChannel(target: LaneTarget): Promise<LaneTarget> {
    const { sessionId, context } = target
    if (context.native || !context.runtimeDraft) return target
    const selectedChannel = engineRuntimeChannel(sessionId)
    const draft = engineDraft(sessionId)
    const selectedCapabilityFingerprint = collectSessionCapabilityFingerprint()
    if (
      !draft
      || (
        sameRuntimeChannel(draft.attachedChannel, selectedChannel)
        && (
          draft.attachedCapabilityFingerprint === undefined
          || draft.attachedCapabilityFingerprint === selectedCapabilityFingerprint
        )
      )
    ) return target
    if (tab.value.race?.engineSwitchLocked) {
      throw new Error(i18n.global.t('engine.draftChannelLocked'))
    }
    if (!context.project) throw new Error(i18n.global.t('common.runtimeUnavailable'))

    const config = engineRunConfig(sessionId)
    const replacement = await rebindDraftChannel({
      sessionId,
      draft,
      selectedChannel,
      selectedCapabilityFingerprint,
      options: engineRuntimeOptions(sessionId),
      config,
    }, {
      createSession,
      sessionId: sessionUiId,
      stageDraft: stageEngineDraft,
      saveConfig: setEngineRunConfig,
      replaceSession: (source, next) => replaceRaceLaneSession(tab.value.id, source, next),
      discardDraft: discardStagedSession,
      replacementError: () => new Error(i18n.global.t('common.runtimeUnavailable')),
    })
    if (!replacement) return target
    return {
      sessionId: replacement.sessionId,
      context: laneContext(replacement.sessionId),
    }
  }

  function projectCwd(project: Project): string | null {
    return project.sessions.find(session => session.cwd)?.cwd ?? project.source_path ?? null
  }

  function projectForEngine(engine: EngineDescriptor, cwd: string): ProjectRef {
    return projects.value.find(project => project.reference
      && project.engine
      && sameInstance(project.engine, engine.instance)
      && projectCwd(project) === cwd)?.reference
      ?? { engine: engine.instance, nativeId: cwd }
  }

  function defaultChannelForEngine(engine: EngineDescriptor): string | null {
    const engineId = engine.instance.engineId
    if (engineId !== 'claude-code' && engineId !== 'codex') return null
    const channelId = defaultSessionChannels.value[engineId]
    if (!channelId) return null
    const channel = channels.value.find(item => item.id === channelId)
    return channel?.enabled
      && channel.scope !== 'agent-only'
      && channelSupportsEngine(channel, engineId)
      ? channelId
      : null
  }

  const switchableEngines = computed(() => engines.value.filter(engine =>
    engine.enabled
    && !!engine.capabilities.runtime?.create
    && health.value[instanceKey(engine.instance)]?.runtime.available === true))

  const canSwitchRaceEngine = computed(() =>
    !tab.value.race?.engineSwitchLocked && switchableEngines.value.length > 1)

  function currentLaneEngine(sessionId: string, choices: EngineDescriptor[]): EngineDescriptor | null {
    const context = laneContext(sessionId)
    if (context.reference) {
      return choices.find(engine => sameInstance(engine.instance, context.reference!.engine)) ?? null
    }
    if (context.native) {
      return choices.find(engine => usesNativeSessionSurface(engine.instance)) ?? null
    }
    return null
  }

  async function switchLaneEngine(sessionId: string) {
    if (raceMutationLoading.value || broadcasting.value) return
    const race = tab.value.race
    const choices = switchableEngines.value
    if (!race || race.engineSwitchLocked || choices.length < 2) return
    const current = currentLaneEngine(sessionId, choices)
    const currentIndex = current
      ? choices.findIndex(engine => sameInstance(engine.instance, current.instance))
      : -1
    const target = choices[(currentIndex + 1) % choices.length]
    if (!target || (current && sameInstance(target.instance, current.instance))) return

    raceError.value = null
    raceMutationLoading.value = true
    let replacementSessionId: string | null = null
    try {
      await refreshChannels()
      if (usesNativeSessionSurface(target.instance)) {
        replacementSessionId = stageDraftSession(race.cwd)
      } else {
        const project = projectForEngine(target, race.cwd)
        const attachedChannel = defaultChannelForEngine(target)
        const created = await createSession(
          project,
          race.cwd,
          attachedChannel ? { channelId: attachedChannel } : {},
        )
        replacementSessionId = sessionUiId(created.session)
        stageEngineDraft(replacementSessionId, {
          reference: created.session,
          project,
          engineName: target.displayName,
          cwd: race.cwd,
          attachedChannel,
          attachedCapabilityFingerprint: created.capabilityFingerprint,
        })
        setEngineRunConfig(replacementSessionId, {
          model: null,
          effort: null,
          serviceTier: null,
          channelId: attachedChannel,
          modelOverridden: false,
          effortOverridden: false,
        })
      }
      if (!replaceRaceLaneSession(tab.value.id, sessionId, replacementSessionId)) {
        discardStagedSession(replacementSessionId)
        replacementSessionId = null
        throw new Error(i18n.global.t('common.runtimeUnavailable'))
      }
    } catch (error) {
      if (replacementSessionId && !tab.value.race?.lanes.some(lane => lane.sessionId === replacementSessionId)) {
        discardStagedSession(replacementSessionId)
      }
      raceError.value = errorMessage(error)
    } finally {
      raceMutationLoading.value = false
    }
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

    let targets: LaneTarget[] = race.lanes.map(lane => ({
      sessionId: lane.sessionId,
      context: laneContext(lane.sessionId),
    }))
    const initialNativeRace = targets.every(target => target.context.native)
    if (initialNativeRace) {
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
      // thread/start 已绑定渠道；首条消息前改渠道时，以新渠道重建空白 lane。
      // 此时没有历史可丢失，原位替换也不会形成发送后的热切换。
      targets = await Promise.all(targets.map(rebindRuntimeDraftChannel))
      const nativeRace = targets.every(target => target.context.native)
      const hasNativeLane = targets.some(target => target.context.native)
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

      if (hasNativeLane) await Promise.all([refreshChannels(), refreshCliDefaults(race.cwd)])
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
          fastMode: rc.launch.fastMode,
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
      if (results.some(result => result.status === 'fulfilled')) {
        lockRaceEngineSelection(tab.value.id)
      }
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
        const attachedChannel = context.runtimeDraft
          ? context.runtimeDraftChannel ?? null
          : engineRuntimeChannel(sourceLane.sessionId)
        const options = {
          ...engineRuntimeOptions(sourceLane.sessionId),
          ...(attachedChannel ? { channelId: attachedChannel } : {}),
        }
        const created = context.runtimeDraft
          ? await createSession(context.project, context.cwd, options)
          : await forkSession(context.reference, null, options)
        const sessionId = sessionUiId(created.session)
        stageEngineDraft(sessionId, {
          reference: created.session,
          project: context.project,
          engineName: context.engineName,
          cwd: context.cwd,
          attachedChannel,
          attachedCapabilityFingerprint: created.capabilityFingerprint,
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
    raceError.value = null
    raceMutationLoading.value = true
    const replacementSessionIds: string[] = []
    try {
      const sourceLanes = [...race.lanes]
      for (const lane of sourceLanes) {
        const context = laneContext(lane.sessionId)
        if (context.native) {
          const sessionId = stageDraftSession(race.cwd)
          inheritRunSettings(lane.sessionId, sessionId)
          replacementSessionIds.push(sessionId)
        } else {
          if (!context.project) throw new Error(i18n.global.t('common.runtimeUnavailable'))
          const runtime = await createSession(context.project, race.cwd, engineRuntimeOptions(lane.sessionId))
          const sessionId = sessionUiId(runtime.session)
          stageEngineDraft(sessionId, {
            reference: runtime.session,
            project: context.project,
            engineName: context.engineName,
            cwd: race.cwd,
            attachedChannel: engineRuntimeChannel(lane.sessionId),
            attachedCapabilityFingerprint: runtime.capabilityFingerprint,
          })
          inheritEngineRunConfig(lane.sessionId, sessionId)
          replacementSessionIds.push(sessionId)
        }
      }
      resetRaceLanes(tab.value.id, replacementSessionIds)
    } catch (error) {
      for (const sessionId of replacementSessionIds) discardStagedSession(sessionId)
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
    canSwitchRaceEngine,
    anyStreaming,
    streamingCount,
    broadcastSend,
    stopAll,
    forkNewLane,
    resetAllLanes,
    switchLaneEngine,
  }
}

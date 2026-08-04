<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useI18n } from 'vue-i18n'
import { relativeTime, type SessionSummary } from '@/types'
import type { ConversationRecord, EngineSegment, ModelDescriptor, RuntimeEventEnvelope, RuntimeSnapshot, SessionActions } from '@/engines/types'
import { attachSession, interruptTurn, listModels, loadTimeline, respondInteraction, runtimeSnapshots, sessionActions, startTurn, steerTurn } from '@/engines/client'
import { sameInstance } from '@/engines/identity'
import { buildEngineAsyncTasks } from '@/engines/asyncTasks'
import { resolveEnginePresentation } from '@/engines/presentation'
import EngineConversationGroup from './EngineConversationGroup.vue'
import EngineAsyncTaskPanel from './EngineAsyncTaskPanel.vue'
import SessionSurface from '@/components/session/SessionSurface.vue'
import SessionComposer from '@/components/session/SessionComposer.vue'
import SessionComposerField from '@/components/session/SessionComposerField.vue'
import SessionViewport from '@/components/session/SessionViewport.vue'
import SessionContentState from '@/components/session/SessionContentState.vue'
import SessionBackToBottom from '@/components/session/SessionBackToBottom.vue'
import SessionInteractionPanel from '@/components/session/SessionInteractionPanel.vue'
import SessionInteractionCard from '@/components/session/SessionInteractionCard.vue'
import SessionReadonlyBar from '@/components/session/SessionReadonlyBar.vue'
import SessionIdentityBar from '@/components/session/SessionIdentityBar.vue'
import SessionToolbar from '@/components/topbar/SessionToolbar.vue'
import SessionTokenBreakdown from '@/components/topbar/SessionTokenBreakdown.vue'
import RunConfigCapsule from '@/components/topbar/RunConfigCapsule.vue'
import { useSessionMeta } from '@/composables/useSessionMeta'
import { useWorkbench } from '@/composables/useWorkbench'
import { useUiState } from '@/composables/useUiState'
import { useProjects } from '@/composables/useProjects'
import { useSessions } from '@/composables/useSessions'
import { useConfirm } from '@/composables/useConfirm'
import { engineRunConfig, setEngineRunConfig, type EngineCapsuleConfig } from '@/engines/runConfig'
import { channelSupportsEngine, engineChannelBinding, engineProviderIdFromSource, OFFICIAL_CHANNEL_ID, refreshChannels, useChannels } from '@/composables/useChannels'

const props = withDefaults(defineProps<{
  session: SessionSummary
  mode?: 'archive' | 'workbench'
  hideInput?: boolean
}>(), { mode: 'archive', hideInput: false })

const { t, locale } = useI18n()
const records = ref<ConversationRecord[]>([])
const liveRecords = ref<ConversationRecord[]>([])
const loading = ref(false)
const attaching = ref(false)
const sending = ref(false)
const error = ref<string | null>(null)
const input = ref('')
const editingMeta = ref(false)
const titleDraft = ref('')
const tagsDraft = ref('')
const snapshot = ref<RuntimeSnapshot | null>(null)
const runtimeId = ref<unknown>(null)
const models = ref<ModelDescriptor[]>([])
const actions = ref<SessionActions | null>(null)
const selectedModel = ref<string | null>(null)
const selectedEffort = ref<string | null>(null)
const modelOverridden = ref(false)
const effortOverridden = ref(false)
const selectedChannel = ref<string | null>(null)
const attachedChannel = ref<string | null | undefined>(undefined)
const runConfigSyncing = ref(false)
const asyncPanelOpen = ref(false)
const menuOpen = ref(false)
const viewportElement = ref<HTMLElement | null>(null)
const followTimeline = ref(true)
const { getMeta, updateMeta } = useSessionMeta()
const { openSession, removeSession, findSession } = useWorkbench()
const { switchSection } = useUiState()
const { loadProjects } = useProjects()
const { selectSession } = useSessions()
const { confirm } = useConfirm()
const { channels } = useChannels()
let unlistenSnapshot: UnlistenFn | null = null
let unlistenEvent: UnlistenFn | null = null
let recoveringSnapshot = false

const reference = computed(() => props.session.reference)
const nativeSessionId = computed(() => props.session.native_id || props.session.id)
const allRecords = computed(() => [...records.value, ...liveRecords.value])
const asyncTasks = computed(() => buildEngineAsyncTasks(allRecords.value))
const latestUsage = computed(() => [...allRecords.value]
  .reverse()
  .find(record => record.usage)?.usage ?? null)
const usedContextTokens = computed(() => {
  const usage = latestUsage.value
  return usage ? usage.inputTokens + (usage.cachedInputTokens ?? 0) : 0
})
const contextCapacity = computed(() => props.session.context_window ?? 0)
const enginePresentation = computed(() => resolveEnginePresentation(
  props.session.engine?.engineId,
  props.session.engine_name,
))
const engineAccent = computed(() => enginePresentation.value.accent)
const engineAccentColor = computed(() => `var(--${engineAccent.value})`)
const timelineModel = computed(() => {
  const value = [...allRecords.value]
    .reverse()
    .map(record => record.sourceMeta.model)
    .find(model => typeof model === 'string' && !!model.trim())
  return typeof value === 'string' ? value : props.session.model
})
const timelineEffort = computed(() => {
  const value = [...allRecords.value]
    .reverse()
    .map(record => record.sourceMeta.effort)
    .find(effort => typeof effort === 'string' && !!effort.trim())
  return typeof value === 'string' ? value : null
})
const sessionEngineId = computed(() => props.session.engine?.engineId ?? 'unknown')
const timelineProvider = computed(() => engineProviderIdFromSource(
  props.session.source_meta,
  sessionEngineId.value,
))
const providerChannel = computed(() => channels.value.find(channel =>
  channel.enabled
  && channelSupportsEngine(channel, sessionEngineId.value)
  && engineChannelBinding(channel, sessionEngineId.value)?.providerId === timelineProvider.value,
) ?? null)
const activeChannel = computed(() => selectedChannel.value
  ? channels.value.find(channel => channel.id === selectedChannel.value) ?? null
  : null)
const effectiveChannel = computed(() => activeChannel.value ?? providerChannel.value)
const activeChannelBinding = computed(() => engineChannelBinding(effectiveChannel.value, sessionEngineId.value))
const capsuleModels = computed(() => {
  const descriptors = models.value.map(model => ({
    id: model.model,
    label: model.displayName,
    hidden: model.hidden,
    defaultEffort: model.defaultEffort,
    efforts: model.efforts,
  }))
  const configured = activeChannelBinding.value
  const extras = [configured?.defaultModel, ...(configured?.availableModels ?? [])]
    .filter((model): model is string => !!model)
    .filter(model => !descriptors.some(item => item.id === model))
    .map(model => ({
      id: model,
      label: model,
      hidden: false,
      defaultEffort: configured?.defaultEffort ?? null,
      efforts: ['low', 'medium', 'high', 'xhigh'].map(id => ({ id, description: null })),
    }))
  return [...descriptors, ...extras]
})
const capsuleConfig = computed<EngineCapsuleConfig>(() => ({
  engineId: sessionEngineId.value,
  engineName: enginePresentation.value.displayName,
  channelId: selectedChannel.value,
  inheritedChannelLabel: providerChannel.value?.name ?? timelineProvider.value,
  model: selectedModel.value,
  effort: selectedEffort.value,
  modelOverridden: modelOverridden.value,
  effortOverridden: effortOverridden.value,
  defaultModel: activeChannelBinding.value?.defaultModel ?? null,
  defaultEffort: activeChannelBinding.value?.defaultEffort ?? null,
  models: capsuleModels.value,
}))
const conversationGroups = computed(() => {
  const groups: Array<{
    key: string
    turnId: string | null
    records: ConversationRecord[]
    dayLabel: string | null
  }> = []
  for (const record of allRecords.value) {
    const current = groups[groups.length - 1]
    const startsNewTurn = !current
      || (!!record.turnId && record.turnId !== current.turnId)
      || (record.role === 'user' && current.records.some(item => item.role === 'user'))
    if (startsNewTurn) {
      groups.push({
        key: record.turnId || record.id,
        turnId: record.turnId,
        records: [record],
        dayLabel: null,
      })
    } else {
      current.records.push(record)
    }
  }
  let previousDay: string | null = null
  const thisYear = new Date().getFullYear()
  for (const group of groups) {
    const timestamp = group.records.find(record => record.role === 'user')?.timestamp
      ?? group.records.find(record => record.timestamp)?.timestamp
    const date = timestamp ? new Date(timestamp) : null
    if (!date || Number.isNaN(date.getTime())) continue
    const day = `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`
    if (day === previousDay) continue
    previousDay = day
    const options: Intl.DateTimeFormatOptions = { month: 'long', day: 'numeric', weekday: 'short' }
    if (date.getFullYear() !== thisYear) options.year = 'numeric'
    group.dayLabel = date.toLocaleDateString(locale.value, options)
  }
  return groups
})
const interactive = computed(() => props.mode === 'workbench' && !!reference.value)
const canSend = computed(() => interactive.value && actions.value?.send.available === true)
const runtimeUnavailableReason = computed(() => {
  const reason = actions.value?.send.reasonCode ?? actions.value?.resume.reasonCode
  return reason ? t(reason, t('common.runtimeUnavailable')) : t('common.runtimeUnavailable')
})
const isBusy = computed(() => snapshot.value?.phase === 'running' || snapshot.value?.phase === 'awaitingInteraction' || sending.value)
const activeTurnId = computed(() => snapshot.value?.activeTurnId ?? null)
const pendingInteractions = computed(() => snapshot.value?.pendingInteractions ?? [])
const starred = computed(() => !!getMeta(props.session.id)?.starred)
const resolvedTitle = computed(() => getMeta(props.session.id)?.title
  || props.session.title
  || props.session.first_user_message
  || props.session.native_id
  || t('session.noTitleSession'))
const tags = computed(() => getMeta(props.session.id)?.tags ?? [])
const resumeUnavailableReason = computed(() => {
  const reason = actions.value?.resume.reasonCode
  return reason ? t(reason, t('common.runtimeUnavailable')) : t('common.runtimeUnavailable')
})

function bindViewport(element: HTMLElement | null) {
  viewportElement.value = element
}

function onTimelineScroll(event: Event) {
  const element = event.currentTarget as HTMLElement
  followTimeline.value = element.scrollHeight - element.scrollTop - element.clientHeight < 24
}

function resumeTimelineFollow() {
  followTimeline.value = true
  viewportElement.value?.scrollTo({ top: viewportElement.value.scrollHeight, behavior: 'smooth' })
}

async function toggleStar() {
  menuOpen.value = false
  await updateMeta(props.session.id, { starred: !starred.value }, reference.value)
}

function beginEditMeta() {
  menuOpen.value = false
  titleDraft.value = getMeta(props.session.id)?.title || props.session.title || ''
  tagsDraft.value = tags.value.join(', ')
  editingMeta.value = true
}

async function saveMeta() {
  if (!reference.value) return
  const normalizedTags = [...new Set(tagsDraft.value
    .split(/[,，]/)
    .map(value => value.trim())
    .filter(Boolean))]
  try {
    await updateMeta(props.session.id, {
      title: titleDraft.value.trim(),
      tags: normalizedTags,
    }, reference.value)
    editingMeta.value = false
  } catch (cause) {
    error.value = String(cause)
  }
}

async function openCwd() {
  menuOpen.value = false
  if (!props.session.cwd || actions.value?.openCwd.available !== true) return
  try {
    await invoke('open_in_finder', { path: props.session.cwd })
  } catch (cause) {
    error.value = String(cause)
  }
}

function openInWorkbench() {
  openSession(props.session.id)
  switchSection('workbench')
}

async function softDelete() {
  menuOpen.value = false
  const approved = await confirm(t('common.softDeleteConfirm'), t('common.delete'))
  if (!approved) return
  await updateMeta(props.session.id, { deleted: true, deletedAt: new Date().toISOString() }, reference.value)
  if (findSession(props.session.id)) removeSession(props.session.id)
  selectSession(null)
  await loadProjects()
}

async function copySessionId() {
  menuOpen.value = false
  try {
    await navigator.clipboard.writeText(nativeSessionId.value)
  } catch (cause) {
    error.value = String(cause)
  }
}

async function reloadFromMenu() {
  menuOpen.value = false
  await reload()
}

function ownsSession(candidate: RuntimeSnapshot['session']): boolean {
  return !!reference.value
    && candidate.nativeId === reference.value.nativeId
    && sameInstance(candidate.engine, reference.value.engine)
}

async function reload() {
  if (!reference.value) return
  loading.value = true
  error.value = null
  try {
    const [timeline, resolvedActions] = await Promise.all([
      loadTimeline(reference.value),
      sessionActions(reference.value),
    ])
    records.value = timeline.records
    actions.value = resolvedActions
  } catch (cause) {
    error.value = String(cause)
  } finally {
    loading.value = false
  }
}

async function ensureAttached() {
  if (!reference.value || attaching.value || actions.value?.resume.available !== true) return
  attaching.value = true
  try {
    if (models.value.length === 0) {
      try {
        models.value = await listModels(reference.value.engine)
        const stored = engineRunConfig(props.session.id)
        selectedChannel.value = stored?.channelId
          ?? null
        const defaultModel = models.value.find(model => model.model === stored?.model)
          ?? models.value.find(model => model.model === activeChannelBinding.value?.defaultModel)
          ?? models.value.find(model => model.model === timelineModel.value)
          ?? models.value.find(model => model.isDefault)
          ?? models.value.find(model => !model.hidden)
        selectedModel.value = defaultModel?.model ?? null
        modelOverridden.value = !!stored?.modelOverridden && !!stored.model
        const storedEffortSupported = defaultModel?.efforts.some(item => item.id === stored?.effort) === true
        selectedEffort.value = storedEffortSupported
          ? stored!.effort
          : activeChannelBinding.value?.defaultEffort
            ?? timelineEffort.value
            ?? defaultModel?.defaultEffort
            ?? null
        effortOverridden.value = !!stored?.effortOverridden && storedEffortSupported
      } catch (_) {
        models.value = []
      }
    }
    if (!runtimeId.value || attachedChannel.value !== selectedChannel.value) {
      const attached = await attachSession(reference.value, {
        ...(selectedChannel.value ? { channelId: selectedChannel.value } : {}),
        ...(selectedModel.value ? { model: selectedModel.value } : {}),
      })
      runtimeId.value = attached.runtimeId
      attachedChannel.value = selectedChannel.value
    }
  } catch (cause) {
    error.value = String(cause)
  } finally {
    attaching.value = false
  }
}

async function recoverRuntimeSnapshot() {
  if (recoveringSnapshot) return
  recoveringSnapshot = true
  try {
    const recovered = (await runtimeSnapshots()).find(item => ownsSession(item.session))
    if (recovered) {
      snapshot.value = recovered
      runtimeId.value = recovered.runtimeId
    }
  } catch (_) {
    // 快照恢复失败不覆盖时间线与现有错误；下一次运行事件仍可继续收敛。
  } finally {
    recoveringSnapshot = false
  }
}

async function send() {
  const text = input.value.trim()
  if (!text || !reference.value || sending.value) return
  if (!isBusy.value) await ensureAttached()
  if (!runtimeId.value) {
    error.value = runtimeUnavailableReason.value
    return
  }
  sending.value = true
  error.value = null
  const optimisticId = `pending-user-${Date.now()}`
  liveRecords.value.push({
    id: optimisticId,
    session: reference.value,
    turnId: activeTurnId.value,
    parentId: null,
    role: 'user',
    timestamp: new Date().toISOString(),
    segments: [{ kind: 'text', text }],
    usage: null,
    sourceMeta: {},
  })
  try {
    if (isBusy.value && runtimeId.value && activeTurnId.value && actions.value?.steer.available) {
      await steerTurn(reference.value, runtimeId.value, activeTurnId.value, text)
    } else {
      await startTurn(reference.value, text, {
        ...(selectedModel.value ? { model: selectedModel.value } : {}),
        ...(selectedEffort.value ? { effort: selectedEffort.value } : {}),
      })
    }
    input.value = ''
  } catch (cause) {
    liveRecords.value = liveRecords.value.filter(record => record.id !== optimisticId)
    error.value = String(cause)
  } finally {
    sending.value = false
  }
}

async function interrupt() {
  if (!reference.value || !runtimeId.value || !activeTurnId.value) return
  try {
    await interruptTurn(reference.value, runtimeId.value, activeTurnId.value)
  } catch (cause) {
    error.value = String(cause)
  }
}

function onEngineChannelChange(channelId: string | null) {
  selectedChannel.value = channelId === OFFICIAL_CHANNEL_ID ? null : channelId
  const channel = selectedChannel.value
    ? channels.value.find(item => item.id === selectedChannel.value) ?? null
    : null
  const binding = engineChannelBinding(channel, sessionEngineId.value)
  if (binding?.defaultModel) {
    selectedModel.value = binding.defaultModel
    modelOverridden.value = false
  }
  if (binding?.defaultEffort) {
    selectedEffort.value = binding.defaultEffort
    effortOverridden.value = false
  }
}

function onEngineModelChange(model: string | null) {
  if (model) {
    selectedModel.value = model
    modelOverridden.value = true
    return
  }
  modelOverridden.value = false
  selectedModel.value = activeChannelBinding.value?.defaultModel
    ?? models.value.find(item => item.isDefault)?.model
    ?? models.value.find(item => !item.hidden)?.model
    ?? null
}

function onEngineEffortChange(effort: string | null) {
  effortOverridden.value = effort !== null
  selectedEffort.value = effort
    ?? activeChannelBinding.value?.defaultEffort
    ?? models.value.find(item => item.model === selectedModel.value)?.defaultEffort
    ?? null
}

async function decide(request: RuntimeSnapshot['pendingInteractions'][number], decision: string) {
  try {
    await respondInteraction(request.reference, decision)
  } catch (cause) {
    error.value = String(cause)
  }
}

function applyRuntimeEvent(envelope: RuntimeEventEnvelope) {
  if (!ownsSession(envelope.session)) return
  const event = envelope.event
  if (event.kind === 'itemDelta') {
    const itemId = String(event.itemId)
    let record = liveRecords.value.find(item => item.id === itemId)
    if (!record) {
      record = {
        id: itemId,
        session: envelope.session,
        turnId: String(event.turnId),
        parentId: null,
        role: 'assistant',
        timestamp: envelope.timestamp,
        segments: [],
        usage: null,
        sourceMeta: {
          ...(selectedModel.value ? { model: selectedModel.value } : {}),
          ...(selectedEffort.value ? { effort: selectedEffort.value } : {}),
        },
      }
      liveRecords.value.push(record)
    }
    const segment = event.segment as EngineSegment
    const last = record.segments[record.segments.length - 1]
    if (last?.kind === 'text' && segment.kind === 'text') last.text += segment.text
    else if (last?.kind === 'reasoning' && segment.kind === 'reasoning') last.text += segment.text
    else if (last?.kind === 'commandExecution' && segment.kind === 'commandExecution' && last.id === segment.id) {
      if (!last.command) last.command = segment.command
      if (!last.cwd) last.cwd = segment.cwd
      last.output = `${last.output ?? ''}${segment.output ?? ''}` || null
      last.status = segment.status
    }
    else record.segments.push(segment)
  } else if (event.kind === 'turnCompleted') {
    window.setTimeout(async () => {
      await reload()
      liveRecords.value = []
    }, 80)
  }
}

function onInputKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault()
    void send()
  }
}

watch(() => props.session.id, async () => {
  runConfigSyncing.value = true
  try {
    records.value = []
    liveRecords.value = []
    snapshot.value = null
    runtimeId.value = null
    models.value = []
    selectedModel.value = null
    selectedEffort.value = null
    modelOverridden.value = false
    effortOverridden.value = false
    selectedChannel.value = null
    attachedChannel.value = undefined
    asyncPanelOpen.value = false
    menuOpen.value = false
    await reload()
    if (interactive.value) await ensureAttached()
  } finally {
    runConfigSyncing.value = false
    setEngineRunConfig(props.session.id, {
      model: selectedModel.value,
      effort: selectedEffort.value,
      channelId: selectedChannel.value,
      modelOverridden: modelOverridden.value,
      effortOverridden: effortOverridden.value,
    })
  }
})

watch(() => asyncTasks.value.length, length => {
  if (length === 0) asyncPanelOpen.value = false
})

watch(() => allRecords.value.length, async () => {
  if (!followTimeline.value) return
  await nextTick()
  viewportElement.value?.scrollTo({ top: viewportElement.value.scrollHeight })
})

watch(selectedModel, (model) => {
  const descriptor = models.value.find(item => item.model === model)
  if (descriptor?.efforts.some(item => item.id === selectedEffort.value)) return
  selectedEffort.value = descriptor?.defaultEffort ?? null
  effortOverridden.value = false
})

watch([selectedModel, selectedEffort, selectedChannel, modelOverridden, effortOverridden], ([model, effort, channelId, modelIsOverridden, effortIsOverridden]) => {
  if (runConfigSyncing.value) return
  setEngineRunConfig(props.session.id, {
    model,
    effort,
    channelId,
    modelOverridden: modelIsOverridden,
    effortOverridden: effortIsOverridden,
  })
})

onMounted(async () => {
  unlistenSnapshot = await listen<RuntimeSnapshot>('engine-runtime-snapshot', event => {
    if (ownsSession(event.payload.session)) {
      snapshot.value = event.payload
      runtimeId.value = event.payload.runtimeId
      if (!event.payload.sequenceConsistent) void recoverRuntimeSnapshot()
    }
  })
  unlistenEvent = await listen<RuntimeEventEnvelope[]>('engine-runtime-events', event => {
    for (const envelope of event.payload) applyRuntimeEvent(envelope)
  })
  await Promise.all([reload(), recoverRuntimeSnapshot(), refreshChannels()])
  if (interactive.value) await ensureAttached()
})

onUnmounted(() => {
  unlistenSnapshot?.()
  unlistenEvent?.()
})
</script>

<template>
  <SessionSurface>
    <template #topbar>
      <SessionIdentityBar
        v-if="mode === 'archive'"
        :engine-name="enginePresentation.displayName"
        :title="resolvedTitle"
        :accent="engineAccent"
        :tags="tags"
      />

      <SessionToolbar
        v-model:menu-open="menuOpen"
        :used-context-tokens="usedContextTokens"
        :context-capacity="contextCapacity"
        :accent="engineAccent"
      >
        <template #controls="{ containerWidth }">
          <span v-if="!interactive && timelineModel" class="min-w-0 max-w-48 truncate rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground" :title="timelineModel">{{ timelineModel }}</span>
          <RunConfigCapsule
            v-if="interactive"
            :engine-config="capsuleConfig"
            :narrow="containerWidth < 280"
            @model-change="onEngineModelChange"
            @effort-change="onEngineEffortChange"
            @channel-change="onEngineChannelChange"
          />
          <span v-if="snapshot" class="shrink-0 text-[10px] text-muted-foreground">{{ t(`engine.phase.${snapshot.phase}`) }}</span>
        </template>

        <template #actions>
          <button
            v-if="asyncTasks.length"
            type="button"
            class="inline-flex shrink-0 items-center gap-1 rounded p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            :style="asyncPanelOpen ? {
              color: engineAccentColor,
              background: `color-mix(in srgb, ${engineAccentColor} 10%, transparent)`,
            } : undefined"
            :title="t('engine.async.title')"
            :aria-pressed="asyncPanelOpen"
            @click="asyncPanelOpen = !asyncPanelOpen"
          >
            <span class="i-carbon-lightning h-3.5 w-3.5" />
            <span class="text-[10px] font-semibold tabular-nums">{{ asyncTasks.length }}</span>
          </button>
        </template>

        <template #menu>
            <div class="flex flex-col gap-1 border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
              <button type="button" class="flex items-center gap-1.5 text-left transition-colors hover:text-foreground" :title="t('topbar.copySessionId')" @click="copySessionId">
                <span class="font-mono">{{ nativeSessionId.slice(0, 8) }}</span>
                <span class="i-carbon-copy h-3 w-3" />
              </button>
              <span v-if="timelineModel" class="truncate">{{ timelineModel }}</span>
              <span>{{ relativeTime(session.last_modified) }}</span>
              <SessionTokenBreakdown :total-tokens="session.total_tokens" :subagent-tokens="session.subagent_tokens" />
            </div>
            <button type="button" class="flex w-full items-center gap-2 px-3 py-1.5 text-xs hover:bg-muted" @click="reloadFromMenu">
              <span class="i-carbon-renew h-3.5 w-3.5" />{{ t('topbar.refreshSession') }}
            </button>
            <button type="button" class="flex w-full items-center gap-2 px-3 py-1.5 text-xs hover:bg-muted" @click="beginEditMeta">
              <span class="i-carbon-edit h-3.5 w-3.5" />{{ t('engine.editMetadata') }}
            </button>
            <button type="button" class="flex w-full items-center gap-2 px-3 py-1.5 text-xs hover:bg-muted" @click="toggleStar">
              <span class="h-3.5 w-3.5" :class="starred ? 'i-carbon-star-filled text-primary' : 'i-carbon-star'" />{{ t('common.star') }}
            </button>
            <button v-if="actions?.openCwd.available" type="button" class="flex w-full items-center gap-2 px-3 py-1.5 text-xs hover:bg-muted" @click="openCwd">
              <span class="i-carbon-folder h-3.5 w-3.5" />{{ t('engine.openCwd') }}
            </button>
            <button v-if="mode === 'archive'" type="button" class="flex w-full items-center gap-2 px-3 py-1.5 text-xs text-destructive hover:bg-muted" @click="softDelete">
              <span class="i-carbon-trash-can h-3.5 w-3.5" />{{ t('common.delete') }}
            </button>
        </template>
      </SessionToolbar>

      <form v-if="editingMeta" class="shrink-0 grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] items-end gap-2 border-b border-border bg-card px-3 py-2" @submit.prevent="saveMeta">
      <label class="min-w-0 text-[10px] text-muted-foreground">
        <span class="mb-1 block">{{ t('engine.metadataTitle') }}</span>
        <input v-model="titleDraft" class="w-full rounded border border-input bg-background px-2 py-1.5 text-xs text-foreground outline-none focus:border-ring" />
      </label>
      <label class="min-w-0 text-[10px] text-muted-foreground">
        <span class="mb-1 block">{{ t('engine.metadataTags') }}</span>
        <input v-model="tagsDraft" class="w-full rounded border border-input bg-background px-2 py-1.5 text-xs text-foreground outline-none focus:border-ring" :placeholder="t('engine.metadataTagsPlaceholder')" />
      </label>
      <div class="flex gap-1.5">
        <button type="button" class="rounded border border-border px-2.5 py-1.5 text-xs hover:bg-muted" @click="editingMeta = false">{{ t('common.cancel') }}</button>
        <button type="submit" class="rounded bg-primary px-2.5 py-1.5 text-xs text-primary-foreground">{{ t('common.save') }}</button>
      </div>
      </form>
    </template>

    <SessionViewport :scroll-ref="bindViewport" @scroll="onTimelineScroll">
      <SessionContentState v-if="loading && !records.length">{{ t('common.loading') }}</SessionContentState>
      <SessionContentState v-else-if="!allRecords.length">{{ t('session.noRecords') }}</SessionContentState>
      <div v-else class="space-y-4 pb-2">
        <EngineConversationGroup
          v-for="group in conversationGroups"
          :key="group.key"
          :records="group.records"
          :engine-name="enginePresentation.displayName"
          :model="timelineModel"
          :accent="engineAccent"
          :show-reasoning-summaries="enginePresentation.showReasoningSummaries"
          :day-label="group.dayLabel"
        />
      </div>
      <SessionContentState v-if="error" tone="danger">{{ error }}</SessionContentState>
      <SessionBackToBottom v-if="!followTimeline" @click="resumeTimelineFollow" />
    </SessionViewport>

    <template #interaction>
      <SessionInteractionPanel v-if="interactive && pendingInteractions.length">
        <SessionInteractionCard
          v-for="request in pendingInteractions"
          :key="request.reference.requestId"
          :danger="request.options.some(option => option.dangerous)"
          role="alertdialog"
          :aria-label="request.title || t('engine.approvalRequired')"
        >
          <div class="flex items-center gap-2 border-b border-border px-3 py-2">
            <span class="i-carbon-locked h-4 w-4 shrink-0 text-muted-foreground" />
            <div class="text-sm font-medium">{{ request.title || t('engine.approvalRequired') }}</div>
          </div>
          <pre class="max-h-48 overflow-auto whitespace-pre-wrap px-3 py-2 text-[10px] text-muted-foreground">{{ JSON.stringify(request.payload, null, 2) }}</pre>
          <div class="flex flex-wrap items-center gap-1.5 border-t border-border px-3 py-2">
            <button
              v-for="option in request.options"
              :key="option.id"
              type="button"
              class="rounded border border-border px-2.5 py-1 text-xs transition-colors hover:bg-muted"
              :class="option.dangerous ? 'text-destructive' : 'text-foreground'"
              @click="decide(request, option.id)"
            >
              {{ t(`engine.decision.${option.id}`, option.label) }}
            </button>
          </div>
        </SessionInteractionCard>
      </SessionInteractionPanel>
    </template>

    <template #input>
      <SessionComposer v-if="canSend && !hideInput">
        <template #field="{ fieldClass }">
          <SessionComposerField
            v-model="input"
            :class="fieldClass"
            :placeholder="t('engine.inputPlaceholder')"
            :disabled="attaching"
            @keydown="onInputKeydown"
          />
        </template>
        <template #actions="{ primaryActionClass, dangerActionClass }">
          <button
            v-if="isBusy && activeTurnId && actions?.steer.available"
            type="button"
            :class="primaryActionClass"
            :disabled="!input.trim() || sending"
            @click="send"
          >
            {{ t('engine.steer') }}
          </button>
          <button
            v-if="isBusy && activeTurnId && actions?.interrupt.available !== false"
            type="button"
            :class="dangerActionClass"
            @click="interrupt"
          >
            {{ t('engine.interrupt') }}
          </button>
          <button
            v-if="!isBusy"
            type="button"
            :class="primaryActionClass"
            :disabled="!input.trim() || attaching || sending"
            @click="send"
          >
            {{ t('engine.send') }}
          </button>
        </template>
      </SessionComposer>
      <div v-else-if="interactive && !hideInput" class="shrink-0 border-t border-border bg-card px-3 py-2 text-center text-xs text-muted-foreground">{{ runtimeUnavailableReason }}</div>
    </template>

    <template #footer>
      <SessionReadonlyBar v-if="mode === 'archive'" :label="t('session.readonlyPreview')">
        <button
          type="button"
          class="shrink-0 rounded bg-primary px-2.5 py-1 text-xs text-primary-foreground transition-shadow hover:shadow-paper disabled:cursor-not-allowed disabled:opacity-45"
          :disabled="actions?.resume.available !== true"
          :title="actions?.resume.available === true ? t('session.openInWorkbench') : resumeUnavailableReason"
          @click="openInWorkbench"
        >
          {{ t('session.openInWorkbench') }}
        </button>
      </SessionReadonlyBar>
    </template>

    <template #side-panel>
      <section v-if="asyncPanelOpen && asyncTasks.length && reference" class="h-full min-w-72 w-[38%] max-w-md shrink-0 border-l border-border">
        <EngineAsyncTaskPanel
          :session="reference"
          :engine-name="enginePresentation.displayName"
          :tasks="asyncTasks"
          :accent="engineAccent"
          @close="asyncPanelOpen = false"
        />
      </section>
    </template>
  </SessionSurface>
</template>

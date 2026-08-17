<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, provide, ref, shallowRef, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useI18n } from 'vue-i18n'
import { relativeTime, type SessionSummary } from '@/types'
import type { ConversationRecord, InteractionRequest, ModelDescriptor, RuntimeEventEnvelope, RuntimeInputItem, RuntimeSnapshot, SessionActions, SessionRef } from '@/engines/types'
import { attachSession, createSession, forkSession, interruptTurn, listModels, loadTimeline, respondInteraction, runtimeSnapshots, sessionActions, startTurnWithInput } from '@/engines/client'
import type { SourceChangeEnvelope } from '@/engines/events'
import { sameInstance } from '@/engines/identity'
import { sessionUiId } from '@/engines/integration'
import { buildEngineAsyncTasks } from '@/engines/asyncTasks'
import { resolveEnginePresentation } from '@/engines/presentation'
import { bindOptimisticUserTurn, composeRuntimeTimeline, createOptimisticUserRecord, hasLiveTurn, reconcileLiveRecords, reduceRuntimeTimeline, reduceRuntimeVisualActivity, syncRuntimeVisualActivity } from '@/engines/runtimeTimeline'
import { bindLatestRuntimeOptimisticInput, reconcileRuntimeOptimisticInputs, useRuntimeOptimisticInputs } from '@/engines/runtimeOptimisticInput'
import EngineConversationGroup from './EngineConversationGroup.vue'
import EngineSegmentBlock from './EngineSegmentBlock.vue'
import EngineAsyncTaskPanel from './EngineAsyncTaskPanel.vue'
import SessionSurface from '@/components/session/SessionSurface.vue'
import SessionComposer from '@/components/session/SessionComposer.vue'
import SessionComposerField from '@/components/session/SessionComposerField.vue'
import SessionComposerAttachments from '@/components/session/SessionComposerAttachments.vue'
import SessionComposerQueue, { type ComposerQueueItem } from '@/components/session/SessionComposerQueue.vue'
import SessionSidePanel from '@/components/session/SessionSidePanel.vue'
import { shouldSubmitComposer } from '@/components/session/composerAction'
import SessionViewport from '@/components/session/SessionViewport.vue'
import SessionContentState from '@/components/session/SessionContentState.vue'
import SessionBackToBottom from '@/components/session/SessionBackToBottom.vue'
import SessionTypingIndicator from '@/components/session/SessionTypingIndicator.vue'
import SessionInteractionPanel from '@/components/session/SessionInteractionPanel.vue'
import SessionApprovalCard, { type SessionApprovalOption } from '@/components/session/SessionApprovalCard.vue'
import SessionReadonlyBar from '@/components/session/SessionReadonlyBar.vue'
import SessionIdentityBar from '@/components/session/SessionIdentityBar.vue'
import ConversationUserMessage from '@/components/session/ConversationUserMessage.vue'
import SessionBannerOverlay from '@/components/session/SessionBannerOverlay.vue'
import SessionToolbar from '@/components/topbar/SessionToolbar.vue'
import SessionTokenBreakdown from '@/components/topbar/SessionTokenBreakdown.vue'
import RunConfigCapsule from '@/components/topbar/RunConfigCapsule.vue'
import { triggerMetaGeneration, useSessionMeta } from '@/composables/useSessionMeta'
import { clearHint, getHint, requestHint } from '@/composables/usePermissionHints'
import { useWorkbench } from '@/composables/useWorkbench'
import { useUiState } from '@/composables/useUiState'
import { useProjects } from '@/composables/useProjects'
import { useSessions } from '@/composables/useSessions'
import { useConfirm } from '@/composables/useConfirm'
import { useRuntimeDeltaShaper } from '@/composables/useRuntimeDeltaShaper'
import { useImageInput, type PendingImage } from '@/composables/useImageInput'
import { useSessionSidePanelHost } from '@/composables/useSessionSidePanelHost'
import { SESSION_FILE_ROOT } from '@/composables/useSessionFileLinks'
import { useStickyUserPrompt } from '@/composables/useStickyUserPrompt'
import { TOOL_FOLD_INTERACTION, provideToolFoldState, useToolDisplayMode } from '@/composables/useToolDisplay'
import { engineRunConfig, inheritEngineRunConfig, isFastServiceTierUnavailableError, resolveFastServiceTier, resolveInitialEngineChannel, setEngineRunConfig, type EngineCapsuleConfig } from '@/engines/runConfig'
import { rebindDraftChannel, sameRuntimeChannel, type DraftChannelReplacement } from '@/engines/draftChannel'
import { channelSupportsEngine, engineChannelBinding, engineChannelFromProvider, engineProviderIdFromSource, OFFICIAL_CHANNEL_ID, refreshChannels, useChannels, type SessionEngineId } from '@/composables/useChannels'
import { resolveTool } from '@/components/blocks/tools'
import { groupConversationRecords } from '@/engines/conversationGroups'
import { isRenderableEngineSegment } from '@/engines/processGroups'
import { measureElement as measureVirtualElement, useVirtualizer, type Virtualizer } from '@tanstack/vue-virtual'
import { useVirtualizationSettings } from '@/composables/useVirtualizationSettings'
import { hasUpwardScrollRange, shouldCompensateVirtualItemSizeChange, shouldDetachScrollFollowAfterMovement } from '@/lib/sessionScrollPolicy'
import { collectSessionCapabilityFingerprint, useHtmlVisual } from '@/features'
import SlashCommandPanel from '@/components/SlashCommandPanel.vue'
import SlashHelpCard from '@/components/SlashHelpCard.vue'
import { useComposerCommands } from '@/composables/useComposerCommands'
import {
  composerPrefix,
  formatCommandInvocation,
  getAllCommands,
  parseCommand,
  shouldTriggerPanel,
  type SlashCommand,
} from '@/composables/useSlashCommands'

const props = withDefaults(defineProps<{
  session: SessionSummary
  mode?: 'archive' | 'workbench'
  hideInput?: boolean
}>(), { mode: 'archive', hideInput: false })
provide(SESSION_FILE_ROOT, computed(() => props.session.cwd))

const { t, locale } = useI18n()
const records = ref<ConversationRecord[]>([])
const liveRecords = ref<ConversationRecord[]>([])
const loading = ref(false)
const attaching = ref(false)
const resolvingWriterConflict = ref(false)
const sending = ref(false)
const preparingInput = ref(false)
const interrupting = ref(false)
const error = ref<string | null>(null)
const commandError = ref<string | null>(null)
const input = ref('')
const cursorPos = ref(0)
const showHelpCard = ref(false)
const clearedRecordIds = ref(new Set<string>())
const detailRootRef = ref<HTMLElement>()
const composerFieldRef = ref<InstanceType<typeof SessionComposerField>>()
const textareaRef = computed(() => composerFieldRef.value?.element ?? null)
const editingMeta = ref(false)
const titleDraft = ref('')
const tagsDraft = ref('')
const snapshot = ref<RuntimeSnapshot | null>(null)
const visualActiveTurnId = ref<string | null>(null)
const runtimeId = ref<unknown>(null)
const models = ref<ModelDescriptor[]>([])
const actions = ref<SessionActions | null>(null)
const selectedModel = ref<string | null>(null)
const selectedEffort = ref<string | null>(null)
const selectedServiceTier = ref<string | null>(null)
const fastModeNotice = ref<string | null>(null)
const modelOverridden = ref(false)
const effortOverridden = ref(false)
const selectedChannel = ref<string | null>(null)
const attachedChannel = ref<string | null | undefined>(undefined)
const runtimeProviderId = ref<string | null>(null)
const attachedCapabilityFingerprint = ref<string | undefined>(undefined)
const runConfigSyncing = ref(false)
const asyncPanelOpen = ref(false)
const menuOpen = ref(false)
const viewportElement = ref<HTMLElement | null>(null)
const timelineContentElement = ref<HTMLElement | null>(null)
const followTimeline = ref(true)
const TIMELINE_BOTTOM_THRESHOLD = 24
const TIMELINE_SCROLL_INTENT_MS = 500
let timelineFollowGeneration = 0
let timelineScrollRequestId = 0
let lastTimelineScrollTop = 0
let timelineDownwardIntentAt = Number.NEGATIVE_INFINITY
let timelineUpwardIntentAt = Number.NEGATIVE_INFINITY
let timelineUpwardSettleTimer: number | null = null
let timelineResizeObserver: ResizeObserver | null = null
const { getMeta, updateMeta, refreshSummary } = useSessionMeta()
const summaryGenerating = ref(false)
const { enabled: htmlVisualEnabled } = useHtmlVisual()
const {
  openSession,
  removeSession,
  findSession,
  engineDraft,
  registerEngineDraft,
  stageEngineDraft,
  discardStagedSession,
  replaceWorkbenchSession,
} = useWorkbench()
const { switchSection } = useUiState()
const { loadProjects } = useProjects()
const { selectSession } = useSessions()
const { confirm, confirmMulti } = useConfirm()
const { channels, defaultSessionChannels, defaultSessionModels, defaultSessionEfforts } = useChannels()
const toolFoldState = provideToolFoldState()
const { toolDisplayModeRevision } = useToolDisplayMode()
provide(TOOL_FOLD_INTERACTION, stopTimelineFollow)
let unlistenSnapshot: UnlistenFn | null = null
let unlistenEvent: UnlistenFn | null = null
let unlistenSourceChange: UnlistenFn | null = null
let recoveringSnapshot = false
let sessionGeneration = 0
let timelineRequestId = 0
let lastAppliedTimelineRequestId = 0
let foregroundRequestId = 0
const completedTurnIds = new Set<string>()
const metadataGeneratedTurnIds = new Set<string>()
const settlementTimers = new Map<string, number>()
const TURN_SETTLEMENT_DELAYS = [0, 100, 250, 600, 1_200, 2_000] as const
let queuedInputSequence = 0
let pendingDraftReplacement: DraftChannelReplacement | null = null

interface QueuedRuntimeInput {
  id: string
  text: string
  imageCount: number
  images: Array<{ id: string; dataUrl: string; mediaType: string }>
  input: RuntimeInputItem[]
}

const queuedInputs = ref<QueuedRuntimeInput[]>([])
const sessionBannerVisible = ref(false)
const sessionBannerResumed = ref(false)
const SESSION_BANNER_MS = 5000
let sessionBannerAnnounced = false
let sessionBannerTimer = 0

function resetSessionBanner() {
  window.clearTimeout(sessionBannerTimer)
  sessionBannerTimer = 0
  sessionBannerAnnounced = false
  sessionBannerVisible.value = false
  sessionBannerResumed.value = false
}

function showSessionBanner(resumed: boolean) {
  sessionBannerAnnounced = true
  sessionBannerResumed.value = resumed
  sessionBannerVisible.value = true
  window.clearTimeout(sessionBannerTimer)
  sessionBannerTimer = window.setTimeout(() => {
    sessionBannerVisible.value = false
  }, SESSION_BANNER_MS)
}

function announceCurrentRuntime() {
  if (sessionBannerAnnounced) return
  showSessionBanner(!engineDraft(props.session.id))
}

function bindDetailRoot(element: HTMLElement | null) {
  detailRootRef.value = element ?? undefined
}

const reference = computed(() => props.session.reference)
const nativeSessionId = computed(() => props.session.native_id || props.session.id)
const runtimeOptimisticInputs = useRuntimeOptimisticInputs(reference)
const allRecords = computed(() => composeRuntimeTimeline(
  records.value,
  [...liveRecords.value, ...runtimeOptimisticInputs.value],
)
  .filter(record => !clearedRecordIds.value.has(record.id)))
const asyncTasks = computed(() => buildEngineAsyncTasks(allRecords.value))
const asyncPanelVisible = computed(() => asyncPanelOpen.value && asyncTasks.value.length > 0 && !!reference.value)
const {
  mounted: sidePanelDom,
  expanded: sidePanelExpanded,
  targetWidth: sidePanelWidth,
} = useSessionSidePanelHost(asyncPanelVisible, {
  rootRef: detailRootRef,
  close: () => { asyncPanelOpen.value = false },
})
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
const commandEngineInstance = computed(() => props.session.engine ?? {
  engineId: sessionEngineId.value,
  instanceId: 'default',
})
const commandCwd = computed(() => props.session.cwd ?? engineDraft(props.session.id)?.cwd ?? null)
const commandCatalogContext = computed(() => ({
  engineId: sessionEngineId.value,
  cwd: commandCwd.value,
}))
const {
  skills: composerSkills,
  commands: composerCommands,
  ready: composerCommandsReady,
  refresh: refreshComposerCommands,
} = useComposerCommands(commandEngineInstance, commandCwd)
const allComposerCommands = computed(() => getAllCommands(
  composerSkills.value,
  composerCommands.value,
  commandCatalogContext.value,
))
const commandPanelVisible = computed(() => shouldTriggerPanel(input.value, cursorPos.value))
const timelineProvider = computed(() => engineProviderIdFromSource(
  props.session.source_meta,
  sessionEngineId.value,
))
const observedProvider = computed(() => runtimeProviderId.value ?? timelineProvider.value)
const providerChannel = computed(() => engineChannelFromProvider(
  channels.value,
  sessionEngineId.value,
  observedProvider.value,
))
const observedChannelId = computed(() => providerChannel.value?.id ?? OFFICIAL_CHANNEL_ID)
const observedChannelLabel = computed(() => providerChannel.value?.name ?? observedProvider.value)

function configuredDefaultChannelId(): string | null {
  if (sessionEngineId.value !== 'claude-code' && sessionEngineId.value !== 'codex') return null
  const id = defaultSessionChannels.value[sessionEngineId.value]
  if (!id) return null
  const channel = channels.value.find(item => item.id === id)
  return channel?.enabled
    && channelSupportsEngine(channel, sessionEngineId.value)
    ? id
    : null
}
const activeChannel = computed(() => selectedChannel.value && selectedChannel.value !== OFFICIAL_CHANNEL_ID
  ? channels.value.find(channel => channel.id === selectedChannel.value) ?? null
  : null)
const effectiveChannel = computed(() => activeChannel.value ?? providerChannel.value)
const activeChannelBinding = computed(() => engineChannelBinding(effectiveChannel.value, sessionEngineId.value))
const configuredChannelId = computed(() => configuredDefaultChannelId() ?? OFFICIAL_CHANNEL_ID)
const usesConfiguredSessionDefault = computed(() => selectedChannel.value === configuredChannelId.value)
const engineDefaultModel = computed(() => usesConfiguredSessionDefault.value
  ? defaultSessionModels.value[sessionEngineId.value as SessionEngineId] ?? null
  : null)
const engineDefaultEffort = computed(() => usesConfiguredSessionDefault.value
  ? defaultSessionEfforts.value[sessionEngineId.value as SessionEngineId] ?? null
  : null)
const capsuleModels = computed(() => {
  const descriptors = models.value.map(model => ({
    id: model.model,
    label: model.displayName,
    hidden: model.hidden,
    defaultEffort: model.defaultEffort,
    efforts: model.efforts,
    defaultServiceTier: model.defaultServiceTier,
    serviceTiers: model.serviceTiers,
  }))
  const configured = activeChannelBinding.value
  const extras = [engineDefaultModel.value, ...(configured?.availableModels ?? [])]
    .filter((model): model is string => !!model)
    .filter(model => !descriptors.some(item => item.id === model))
    .map(model => ({
      id: model,
      label: model,
      hidden: false,
      defaultEffort: engineDefaultEffort.value,
      efforts: ['low', 'medium', 'high', 'xhigh'].map(id => ({ id, description: null })),
      defaultServiceTier: null,
      serviceTiers: [],
    }))
  return [...descriptors, ...extras]
})
const selectedCapsuleModel = computed(() => capsuleModels.value.find(model => model.id === selectedModel.value))
const selectedFastTier = computed(() => {
  if (effectiveChannel.value) return null
  return resolveFastServiceTier(selectedCapsuleModel.value)
})
const fastModeUnavailableReason = computed(() => {
  if (effectiveChannel.value) return t('topbar.fastModeUnavailableChannel')
  if (!selectedFastTier.value) return t('topbar.fastModeUnavailableModel')
  return null
})
const capsuleConfig = computed<EngineCapsuleConfig>(() => ({
  engineId: sessionEngineId.value,
  engineName: enginePresentation.value.displayName,
  showFastMode: sessionEngineId.value === 'codex',
  channelId: selectedChannel.value,
  channelOverridden: selectedChannel.value !== null && selectedChannel.value !== configuredChannelId.value,
  channelPending: attachedChannel.value !== undefined && selectedChannel.value !== attachedChannel.value,
  observedChannelLabel: observedChannelLabel.value,
  model: selectedModel.value,
  effort: selectedEffort.value,
  modelOverridden: modelOverridden.value,
  effortOverridden: effortOverridden.value,
  serviceTier: selectedServiceTier.value,
  fastTier: selectedFastTier.value,
  fastModeUnavailableReason: fastModeUnavailableReason.value,
  defaultModel: engineDefaultModel.value,
  defaultEffort: engineDefaultEffort.value,
  models: capsuleModels.value,
}))
const conversationGroups = computed(() => {
  const groups = groupConversationRecords(allRecords.value).map(group => ({ ...group, dayLabel: null as string | null }))
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
const historicalGroups = computed(() => conversationGroups.value.length > 1
  ? conversationGroups.value.slice(0, -1)
  : [])
const lastConversationGroup = computed(() => {
  const groups = conversationGroups.value
  return groups.length ? groups[groups.length - 1] : null
})
const historicalGroupKeys = computed(() => historicalGroups.value.map(group =>
  `${props.session.id}:${group.key}`,
))
const historicalKeySnapshot = shallowRef<readonly string[]>([])
const historicalKeyExtractor = shallowRef<(index: number) => string>(index => `missing:${index}`)
const historicalGroupHeights = new Map<string, number>()
const historicalVirtualBoxElement = ref<HTMLElement>()
const lastGroupElement = ref<HTMLElement>()
let lastGroupResizeObserver: ResizeObserver | null = null
let stickyUpdateFrame = 0

watch(historicalGroupKeys, (keys) => {
  const previous = historicalKeySnapshot.value
  if (keys.length === previous.length && keys.every((key, index) => key === previous[index])) return
  const snapshot = [...keys]
  historicalKeySnapshot.value = snapshot
  historicalKeyExtractor.value = index => snapshot[index] ?? `${props.session.id}:missing:${index}`
}, { immediate: true, flush: 'sync' })

function measureConversationGroup(
  element: Element,
  entry: ResizeObserverEntry | undefined,
  instance: Virtualizer<HTMLElement, Element>,
): number {
  const size = measureVirtualElement(element, entry, instance)
  const key = instance.options.getItemKey(instance.indexFromElement(element))
  if (size > 0) historicalGroupHeights.set(String(key), size)
  return size
}

const { threshold: virtualizationThreshold } = useVirtualizationSettings()
const { stickyUserPromptFor } = useStickyUserPrompt()
const stickyUserPromptEnabled = computed(() => stickyUserPromptFor(sessionEngineId.value))
const shouldVirtualize = computed(() => historicalGroups.value.length > virtualizationThreshold.value)
const conversationVirtualizer = useVirtualizer(computed(() => ({
  count: historicalGroups.value.length,
  getScrollElement: () => viewportElement.value,
  getItemKey: historicalKeyExtractor.value,
  estimateSize: (index: number) =>
    historicalGroupHeights.get(historicalKeyExtractor.value(index)) ?? 200,
  measureElement: measureConversationGroup,
  gap: 16,
  overscan: 5,
})))

conversationVirtualizer.value.shouldAdjustScrollPositionOnItemSizeChange = (item, delta, instance) =>
  shouldCompensateVirtualItemSizeChange({
    scrollDirection: instance.scrollDirection,
    upwardGestureActive: performance.now() - timelineUpwardIntentAt < 220,
    itemStart: item.start,
    itemSize: item.size,
    scrollOffset: instance.scrollOffset ?? 0,
    delta,
  })

watch(shouldVirtualize, () => void nextTick(() => conversationVirtualizer.value.measure()))
watch(toolDisplayModeRevision, () => void nextTick(() => conversationVirtualizer.value.measure()))
watch(lastGroupElement, (element) => {
  lastGroupResizeObserver?.disconnect()
  lastGroupResizeObserver = null
  if (!element) return
  const rememberHeight = () => {
    const key = element.dataset.groupKey
    const height = element.getBoundingClientRect().height
    if (key && height > 0) historicalGroupHeights.set(key, height)
  }
  rememberHeight()
  lastGroupResizeObserver = new ResizeObserver(rememberHeight)
  lastGroupResizeObserver.observe(element)
})
watch(() => props.session.id, () => {
  historicalGroupHeights.clear()
  void nextTick(() => conversationVirtualizer.value.measure())
})

const STICKY_CARD_GAP = 16
const stickyOverlayElement = ref<HTMLElement | null>(null)
const stickySurfaceElement = ref<HTMLElement | null>(null)
const stickyGroupIndex = ref(-1)
const stickyPushOffset = ref(0)
let stickySurfaceResizeObserver: ResizeObserver | null = null

function visibleGroupUserSegments(group: typeof conversationGroups.value[number]) {
  return group.records
    .filter(record => record.role === 'user')
    .flatMap(record => record.segments)
    .filter(segment => isRenderableEngineSegment(
      segment,
      enginePresentation.value.showThoughtProcess,
    ))
}

function enginePromptElement(owner: HTMLElement | null | undefined): HTMLElement | null {
  return owner?.querySelector<HTMLElement>('.conversation-user-message') ?? null
}

function engineStickySurfaceCard(): HTMLElement | null {
  return enginePromptElement(stickySurfaceElement.value)
}

function stickyRestingClientTop(): number | null {
  const card = engineStickySurfaceCard()
  if (card) return card.getBoundingClientRect().top - stickyPushOffset.value
  return stickyOverlayElement.value?.getBoundingClientRect().top ?? null
}

function groupHasVisibleUser(index: number): boolean {
  const group = conversationGroups.value[index]
  return !!group && visibleGroupUserSegments(group).length > 0
}

function stickyNeighborIndex(from: number, direction: 1 | -1): number {
  for (let index = from; index >= 0 && index < conversationGroups.value.length; index += direction) {
    if (groupHasVisibleUser(index)) return index
  }
  return -1
}

function setStickyGroup(index: number) {
  if (stickyGroupIndex.value === index) return
  stickyPushOffset.value = 0
  stickyGroupIndex.value = index
}

function updateStickyGroup() {
  if (!stickyUserPromptEnabled.value) {
    setStickyGroup(-1)
    return
  }
  const restingTop = stickyRestingClientTop()
  if (restingTop === null) return

  let latestPast = -1
  let firstFuture = Number.POSITIVE_INFINITY
  for (const owner of timelineContentElement.value?.querySelectorAll<HTMLElement>('[data-conversation-index]') ?? []) {
    const index = Number(owner.dataset.conversationIndex)
    const card = enginePromptElement(owner)
    if (!Number.isInteger(index) || !card || !groupHasVisibleUser(index)) continue
    if (card.getBoundingClientRect().top <= restingTop + 0.5) latestPast = Math.max(latestPast, index)
    else firstFuture = Math.min(firstFuture, index)
  }

  if (latestPast >= 0) setStickyGroup(latestPast)
  else if (Number.isFinite(firstFuture)) setStickyGroup(stickyNeighborIndex(firstFuture - 1, -1))
}

const stickyDisplay = computed(() => {
  if (!stickyUserPromptEnabled.value) return null
  const index = stickyNeighborIndex(stickyGroupIndex.value, -1)
  return index >= 0 ? { group: conversationGroups.value[index], index } : null
})
const stickyUserSegments = computed(() => stickyDisplay.value
  ? visibleGroupUserSegments(stickyDisplay.value.group)
  : [])
function stickyDateTimeLabel(value: string | null | undefined): string {
  if (!value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  return date.toLocaleString(locale.value, {
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}
const stickyTimeLabel = computed(() => {
  const timestamp = stickyDisplay.value?.group.records.find(record => record.role === 'user')?.timestamp
  return stickyDateTimeLabel(timestamp)
})
const stickyPreviousIndex = computed(() => stickyDisplay.value
  ? stickyNeighborIndex(stickyDisplay.value.index - 1, -1)
  : -1)
const stickyNextIndex = computed(() => stickyDisplay.value
  ? stickyNeighborIndex(stickyDisplay.value.index + 1, 1)
  : -1)

function engineGroupPromptCard(index: number): HTMLElement | null {
  const owner = timelineContentElement.value?.querySelector<HTMLElement>(`[data-conversation-index="${index}"]`)
  return enginePromptElement(owner)
}

function updateStickyPushOffset() {
  const currentCard = engineStickySurfaceCard()
  if (!stickyDisplay.value || !currentCard) {
    stickyPushOffset.value = 0
    return
  }
  const nextCard = stickyNextIndex.value >= 0
    ? engineGroupPromptCard(stickyNextIndex.value)
    : null
  if (!nextCard) {
    stickyPushOffset.value = 0
    return
  }
  const restingBottom = currentCard.getBoundingClientRect().bottom - stickyPushOffset.value
  stickyPushOffset.value = Math.min(
    0,
    nextCard.getBoundingClientRect().top - STICKY_CARD_GAP - restingBottom,
  )
}

function scheduleStickyUpdate() {
  if (stickyUpdateFrame) return
  stickyUpdateFrame = window.requestAnimationFrame(() => {
    stickyUpdateFrame = 0
    updateStickyGroup()
    updateStickyPushOffset()
  })
}

watch(stickySurfaceElement, element => {
  stickySurfaceResizeObserver?.disconnect()
  stickySurfaceResizeObserver = null
  if (!element) return
  stickySurfaceResizeObserver = new ResizeObserver(scheduleStickyUpdate)
  stickySurfaceResizeObserver.observe(element)
  scheduleStickyUpdate()
})

watch([conversationGroups, stickyUserPromptEnabled, shouldVirtualize], () => {
  void nextTick(scheduleStickyUpdate)
})

function engineElementScrollTop(element: HTMLElement, viewport: HTMLElement): number {
  return element.getBoundingClientRect().top - viewport.getBoundingClientRect().top + viewport.scrollTop
}

function scrollToConversationGroup(index: number) {
  if (index < 0) return
  stopTimelineFollow()
  const viewport = viewportElement.value
  if (!shouldVirtualize.value) {
    const element = timelineContentElement.value?.querySelector<HTMLElement>(`[data-conversation-index="${index}"]`)
    if (!viewport || !element) return
    viewport.scrollTop = engineElementScrollTop(element, viewport)
    return
  }
  if (index < historicalGroups.value.length) conversationVirtualizer.value.scrollToIndex(index, { align: 'start' })
}
const interactive = computed(() => props.mode === 'workbench' && !!reference.value)
const canSend = computed(() => interactive.value && actions.value?.send.available === true)
const imageDropArea = computed<HTMLElement | null | undefined>(() =>
  interactive.value && !props.hideInput ? detailRootRef.value : null,
)
const imageInput = useImageInput({ pasteTarget: textareaRef, dropTarget: imageDropArea })
const runtimeUnavailableReason = computed(() => {
  const reason = actions.value?.send.reasonCode ?? actions.value?.resume.reasonCode
  return reason ? t(reason, t('common.runtimeUnavailable')) : t('common.runtimeUnavailable')
})
const runtimeActive = computed(() => snapshot.value?.phase === 'running' || snapshot.value?.phase === 'awaitingInteraction')
const isBusy = computed(() => runtimeActive.value || sending.value)
const activeTurnId = computed(() => snapshot.value?.activeTurnId ?? null)
const canSendWhileBusy = computed(() => canSend.value)
const composerQueueItems = computed<ComposerQueueItem[]>(() => queuedInputs.value.map(item => ({
  id: item.id,
  text: item.text,
  imageCount: item.imageCount,
})))
const pendingInteractions = computed(() => snapshot.value?.pendingInteractions ?? [])
const starred = computed(() => !!getMeta(props.session.id)?.starred)
const currentSummary = computed(() => getMeta(props.session.id)?.summary)
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

function causeMessage(cause: unknown): string {
  if (typeof cause === 'string') return cause
  if (cause && typeof cause === 'object' && 'message' in cause) {
    const message = (cause as { message?: unknown }).message
    if (typeof message === 'string') return message
  }
  return String(cause)
}

function isActiveWriterConflict(cause: unknown): boolean {
  if (sessionEngineId.value !== 'codex') return false
  if (cause && typeof cause === 'object' && 'kind' in cause) {
    return (cause as { kind?: unknown }).kind === 'conflict'
  }
  return causeMessage(cause).includes('already has an active writer')
}

function bindViewport(element: HTMLElement | null) {
  if (viewportElement.value !== element) {
    clearTimelineUpwardSettleTimer()
    timelineFollowGeneration++
    timelineScrollRequestId++
    lastTimelineScrollTop = element?.scrollTop ?? 0
  }
  viewportElement.value = element
}

function timelineDistanceFromBottom(element: HTMLElement): number {
  return Math.max(0, element.scrollHeight - element.scrollTop - element.clientHeight)
}

function invalidateTimelineScrollRequests() {
  timelineFollowGeneration++
  timelineScrollRequestId++
}

function clearTimelineUpwardSettleTimer() {
  if (timelineUpwardSettleTimer === null) return
  window.clearTimeout(timelineUpwardSettleTimer)
  timelineUpwardSettleTimer = null
}

function timelineUpwardIntentActive(now = performance.now()): boolean {
  const age = now - timelineUpwardIntentAt
  return timelineUpwardIntentAt > timelineDownwardIntentAt
    && age >= 0
    && age <= TIMELINE_SCROLL_INTENT_MS
}

function scheduleTimelineFollowAfterUpwardIntent(intentAt: number) {
  clearTimelineUpwardSettleTimer()
  timelineUpwardSettleTimer = window.setTimeout(() => {
    timelineUpwardSettleTimer = null
    if (!followTimeline.value || timelineUpwardIntentAt !== intentAt) return
    requestTimelineFollow()
  }, TIMELINE_SCROLL_INTENT_MS + 16)
}

function stopTimelineFollow() {
  clearTimelineUpwardSettleTimer()
  followTimeline.value = false
  invalidateTimelineScrollRequests()
}

function resumeTimelineFollowIfAtBottom(element: HTMLElement, intentAt: number): boolean {
  if (
    followTimeline.value
    || viewportElement.value !== element
    || timelineDownwardIntentAt !== intentAt
    || intentAt <= timelineUpwardIntentAt
    || performance.now() - intentAt > TIMELINE_SCROLL_INTENT_MS
    || timelineDistanceFromBottom(element) > TIMELINE_BOTTOM_THRESHOLD
  ) return false
  resumeTimelineFollow()
  return true
}

function onTimelineWheel(event: WheelEvent) {
  if (event.deltaY < 0) {
    const element = viewportElement.value
    // wheel 只暂停写底；等 scroll 证明视口确实离开底部后才显示按钮。
    if (!element || !hasUpwardScrollRange(element)) return
    const intentAt = performance.now()
    timelineUpwardIntentAt = intentAt
    invalidateTimelineScrollRequests()
    scheduleTimelineFollowAfterUpwardIntent(intentAt)
    return
  }
  if (event.deltaY <= 0) return
  const intentAt = performance.now()
  timelineDownwardIntentAt = intentAt
  clearTimelineUpwardSettleTimer()
  const element = viewportElement.value
  if (!element) return
  if (followTimeline.value) {
    requestTimelineFollow()
    return
  }
  // 已在底部时向下滚不会再触发 scroll，直接恢复后续内容跟随。
  if (resumeTimelineFollowIfAtBottom(element, intentAt)) return

  // 触控板的最后一小段惯性位移可能没有形成足够大的 scroll delta；等浏览器
  // 应用本帧原生滚动后再复核一次，确保物理触底必然恢复跟随。
  const generation = timelineFollowGeneration
  requestAnimationFrame(() => {
    if (generation !== timelineFollowGeneration) return
    resumeTimelineFollowIfAtBottom(element, intentAt)
  })
}

function onTimelineScroll(event: Event) {
  scheduleStickyUpdate()
  const element = event.currentTarget as HTMLElement
  const nextScrollTop = element.scrollTop
  const previousScrollTop = lastTimelineScrollTop
  const delta = nextScrollTop - previousScrollTop
  const reachedBottom = timelineDistanceFromBottom(element) <= TIMELINE_BOTTOM_THRESHOLD
  lastTimelineScrollTop = nextScrollTop

  if (followTimeline.value && shouldDetachScrollFollowAfterMovement({
    geometry: element,
    previousScrollTop,
    upwardIntentAt: timelineUpwardIntentAt,
    downwardIntentAt: timelineDownwardIntentAt,
    now: performance.now(),
    intentWindow: TIMELINE_SCROLL_INTENT_MS,
    bottomThreshold: TIMELINE_BOTTOM_THRESHOLD,
  })) {
    stopTimelineFollow()
    return
  }

  // 布局变化也可能把 scrollTop 推到底，只有近期用户向下滚动才恢复跟随。
  if (
    !followTimeline.value
    && reachedBottom
    && delta > 0.5
    && timelineDownwardIntentAt > timelineUpwardIntentAt
    && performance.now() - timelineDownwardIntentAt <= TIMELINE_SCROLL_INTENT_MS
  ) {
    resumeTimelineFollow()
  }
}

function resumeTimelineFollow() {
  clearTimelineUpwardSettleTimer()
  timelineDownwardIntentAt = Number.NEGATIVE_INFINITY
  timelineUpwardIntentAt = Number.NEGATIVE_INFINITY
  invalidateTimelineScrollRequests()
  followTimeline.value = true
  requestTimelineFollow()
}

function resetTimelineFollow() {
  clearTimelineUpwardSettleTimer()
  invalidateTimelineScrollRequests()
  followTimeline.value = true
  timelineDownwardIntentAt = Number.NEGATIVE_INFINITY
  timelineUpwardIntentAt = Number.NEGATIVE_INFINITY
  lastTimelineScrollTop = viewportElement.value?.scrollTop ?? 0
}

function requestTimelineFollow(allowLayoutReset = false) {
  if (!followTimeline.value || (!allowLayoutReset && timelineUpwardIntentActive())) return
  const generation = timelineFollowGeneration
  const requestId = ++timelineScrollRequestId
  const scheduledElement = viewportElement.value
  const scheduledScrollTop = scheduledElement?.scrollTop ?? 0
  void nextTick(() => {
    requestAnimationFrame(() => {
      const element = viewportElement.value
      if (
        !followTimeline.value
        || generation !== timelineFollowGeneration
        || requestId !== timelineScrollRequestId
        || !element
        || (scheduledElement !== null && element !== scheduledElement)
      ) return
      // scroll 事件尚未分发时，也用位置变化识别已经开始的向上阅读。
      if (!allowLayoutReset && shouldDetachScrollFollowAfterMovement({
        geometry: element,
        previousScrollTop: scheduledScrollTop,
        upwardIntentAt: timelineUpwardIntentAt,
        downwardIntentAt: timelineDownwardIntentAt,
        now: performance.now(),
        intentWindow: TIMELINE_SCROLL_INTENT_MS,
        bottomThreshold: TIMELINE_BOTTOM_THRESHOLD,
      })) {
        stopTimelineFollow()
        return
      }
      const target = Math.max(0, element.scrollHeight - element.clientHeight)
      if (target - element.scrollTop > 0.5) element.scrollTop = target
      lastTimelineScrollTop = element.scrollTop
    })
  })
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
    error.value = causeMessage(cause)
  }
}

async function openCwd() {
  menuOpen.value = false
  if (!props.session.cwd || actions.value?.openCwd.available !== true) return
  try {
    await invoke('open_in_finder', { path: props.session.cwd })
  } catch (cause) {
    error.value = causeMessage(cause)
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
    error.value = causeMessage(cause)
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

function isCurrentTarget(target: SessionRef, generation: number): boolean {
  return generation === sessionGeneration && ownsSession(target)
}

function cancelTurnSettlements() {
  for (const timer of settlementTimers.values()) window.clearTimeout(timer)
  settlementTimers.clear()
  completedTurnIds.clear()
  metadataGeneratedTurnIds.clear()
}

function generateMetadataForSettledTurn(turnId: string) {
  if (!interactive.value || !reference.value || metadataGeneratedTurnIds.has(turnId)) return
  metadataGeneratedTurnIds.add(turnId)
  triggerMetaGeneration(reference.value)
}

function reconcileLoadedRecords(timelineRecords: ConversationRecord[]) {
  if (reference.value) reconcileRuntimeOptimisticInputs(reference.value, timelineRecords)
  records.value = timelineRecords
  liveRecords.value = reconcileLiveRecords(
    timelineRecords,
    liveRecords.value,
  )
  for (const turnId of [...completedTurnIds]) {
    if (!hasLiveTurn(liveRecords.value, turnId)) {
      generateMetadataForSettledTurn(turnId)
      completedTurnIds.delete(turnId)
      const timer = settlementTimers.get(turnId)
      if (timer !== undefined) window.clearTimeout(timer)
      settlementTimers.delete(turnId)
    }
  }
}

async function reload(options: { quiet?: boolean } = {}): Promise<boolean> {
  const target = reference.value
  if (!target) return false
  const generation = sessionGeneration
  const requestId = ++timelineRequestId
  if (!options.quiet) {
    foregroundRequestId = requestId
    loading.value = true
    error.value = null
  }
  try {
    const [timeline, resolvedActions] = await Promise.all([
      loadTimeline(target),
      sessionActions(target),
    ])
    if (!isCurrentTarget(target, generation) || requestId < lastAppliedTimelineRequestId) return false
    lastAppliedTimelineRequestId = requestId
    reconcileLoadedRecords(timeline.records)
    actions.value = resolvedActions
    return true
  } catch (cause) {
    if (isCurrentTarget(target, generation)) error.value = causeMessage(cause)
    return false
  } finally {
    if (!options.quiet && requestId === foregroundRequestId && generation === sessionGeneration) {
      loading.value = false
    }
  }
}

async function loadRuntimeConfiguration() {
  const target = reference.value
  if (!target || models.value.length > 0) return
  const generation = sessionGeneration
  const sessionId = props.session.id
  try {
    const loadedModels = await listModels(target.engine)
    if (!isCurrentTarget(target, generation) || props.session.id !== sessionId) return
    models.value = loadedModels
    const stored = engineRunConfig(sessionId)
    const draft = engineDraft(sessionId)
    const draftProvider = engineProviderIdFromSource(draft?.sourceMeta, sessionEngineId.value)
    if (draftProvider) runtimeProviderId.value = draftProvider
    const currentChannelId = draft?.attachedChannel ?? observedChannelId.value
    selectedChannel.value = resolveInitialEngineChannel(
      stored,
      configuredDefaultChannelId(),
      currentChannelId,
    ) ?? OFFICIAL_CHANNEL_ID
    if (attachedChannel.value === undefined) {
      attachedChannel.value = draft?.attachedChannel
        ?? stored?.channelId
        ?? observedChannelId.value
    }
    const defaultModel = models.value.find(model => model.model === stored?.model)
      ?? models.value.find(model => model.model === engineDefaultModel.value)
      ?? models.value.find(model => model.model === timelineModel.value)
      ?? models.value.find(model => model.isDefault)
      ?? models.value.find(model => !model.hidden)
    selectedModel.value = defaultModel?.model ?? null
    modelOverridden.value = !!stored?.modelOverridden && !!stored.model
    const storedEffortSupported = defaultModel?.efforts.some(item => item.id === stored?.effort) === true
    selectedEffort.value = storedEffortSupported
      ? stored!.effort
      : engineDefaultEffort.value
        ?? timelineEffort.value
        ?? defaultModel?.defaultEffort
        ?? null
    effortOverridden.value = !!stored?.effortOverridden && storedEffortSupported
    const availableServiceTiers = new Set(defaultModel?.serviceTiers.map(tier => tier.id) ?? [])
    const requestedServiceTier = stored
      ? stored.serviceTier
      : defaultModel?.defaultServiceTier ?? null
    selectedServiceTier.value = !effectiveChannel.value
      && requestedServiceTier
      && availableServiceTiers.has(requestedServiceTier)
      ? requestedServiceTier
      : null
  } catch (_) {
    if (isCurrentTarget(target, generation)) models.value = []
  }
}

type AttachOutcome = 'attached' | 'writer-conflict' | 'failed'

async function rebindCurrentDraftChannel(): Promise<boolean> {
  const sessionId = props.session.id
  const draft = engineDraft(sessionId)
  const capabilityFingerprint = collectSessionCapabilityFingerprint()
  if (
    !draft
    || (
      sameRuntimeChannel(draft.attachedChannel, selectedChannel.value)
      && (
        draft.attachedCapabilityFingerprint === undefined
        || draft.attachedCapabilityFingerprint === capabilityFingerprint
      )
    )
  ) return true

  const config = {
    model: selectedModel.value,
    effort: selectedEffort.value,
    serviceTier: selectedServiceTier.value,
    channelId: selectedChannel.value,
    modelOverridden: modelOverridden.value,
    effortOverridden: effortOverridden.value,
  }
  const replacement = await rebindDraftChannel({
    sessionId,
    draft,
    selectedChannel: selectedChannel.value,
    selectedCapabilityFingerprint: capabilityFingerprint,
    options: {
      ...(selectedChannel.value ? { channelId: selectedChannel.value } : {}),
      ...(selectedModel.value ? { model: selectedModel.value } : {}),
    },
    config,
  }, {
    createSession,
    sessionId: sessionUiId,
    stageDraft: stageEngineDraft,
    saveConfig: setEngineRunConfig,
    // Vue 会在工作台 id 改写后调度 props watcher；必须在改写前登记，
    // watcher 才能保留本轮选择和新 runtime，而不是按普通切会话清空。
    beforeReplace: value => { pendingDraftReplacement = value },
    replaceSession: replaceWorkbenchSession,
    discardDraft: replacementSessionId => {
      if (pendingDraftReplacement?.sessionId === replacementSessionId) pendingDraftReplacement = null
      discardStagedSession(replacementSessionId)
    },
    replacementError: () => new Error(t('common.runtimeUnavailable')),
  })
  if (!replacement) return true

  await nextTick()
  if (props.session.id !== replacement.sessionId) {
    pendingDraftReplacement = null
    return false
  }
  runtimeId.value = replacement.runtimeId
  runtimeProviderId.value = engineProviderIdFromSource(replacement.sourceMeta, sessionEngineId.value)
  attachedChannel.value = replacement.attachedChannel
  attachedCapabilityFingerprint.value = replacement.attachedCapabilityFingerprint
  return true
}

async function ensureAttached(): Promise<AttachOutcome> {
  if (!reference.value || attaching.value) return 'failed'
  // 已在运行的 runtime 不能 resume：重挂载会清掉 active turn，令后续事件失配。
  if (isBusy.value) return runtimeId.value !== null ? 'attached' : 'failed'
  attaching.value = true
  try {
    await loadRuntimeConfiguration()
    let draft = engineDraft(props.session.id)
    const capabilityFingerprint = collectSessionCapabilityFingerprint()
    while (
      draft
      && (
        !sameRuntimeChannel(draft.attachedChannel, selectedChannel.value)
        || (
          draft.attachedCapabilityFingerprint !== undefined
          && draft.attachedCapabilityFingerprint !== capabilityFingerprint
        )
      )
    ) {
      if (!(await rebindCurrentDraftChannel())) return 'failed'
      draft = engineDraft(props.session.id)
    }
    if (runtimeId.value && draft) {
      // create/fork 草稿记录的是已确认渠道选择；空线程首条消息前尚无
      // rollout，不能重复 resume。已落盘会话没有这份可靠记录，必须走
      // attach 来应用当前渠道，不能把 UI 选择误当成 runtime 真实状态。
      if (attachedChannel.value === undefined) attachedChannel.value = draft.attachedChannel
      if (!runtimeProviderId.value) {
        runtimeProviderId.value = engineProviderIdFromSource(draft.sourceMeta, sessionEngineId.value)
      }
      // 旧版运行期草稿没有该字段。它由同一 capability 收集器创建，按当前值
      // 接管可避免在首轮 rollout 生成前发出必然失败的 thread/resume。
      if (attachedCapabilityFingerprint.value === undefined) {
        attachedCapabilityFingerprint.value = draft.attachedCapabilityFingerprint
          ?? capabilityFingerprint
      }
    }
    if (
      runtimeId.value
      && sameRuntimeChannel(attachedChannel.value ?? null, selectedChannel.value)
      && attachedCapabilityFingerprint.value === capabilityFingerprint
    ) {
      announceCurrentRuntime()
      return 'attached'
    }
    if (actions.value?.resume.available !== true) {
      const reason = actions.value?.resume.reasonCode
      error.value = reason ? t(reason, t('common.runtimeUnavailable')) : t('common.runtimeUnavailable')
      return 'failed'
    }
    if (
      !runtimeId.value
      || !sameRuntimeChannel(attachedChannel.value ?? null, selectedChannel.value)
      || attachedCapabilityFingerprint.value !== capabilityFingerprint
    ) {
      const attached = await attachSession(reference.value, {
        ...(selectedChannel.value ? { channelId: selectedChannel.value } : {}),
        ...(selectedModel.value ? { model: selectedModel.value } : {}),
      })
      runtimeId.value = attached.runtimeId
      runtimeProviderId.value = engineProviderIdFromSource(attached.sourceMeta, sessionEngineId.value)
      attachedChannel.value = selectedChannel.value
      attachedCapabilityFingerprint.value = attached.capabilityFingerprint
      showSessionBanner(!engineDraft(props.session.id))
    }
    return 'attached'
  } catch (cause) {
    if (isActiveWriterConflict(cause)) return 'writer-conflict'
    error.value = causeMessage(cause)
    return 'failed'
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
      visualActiveTurnId.value = syncRuntimeVisualActivity(
        visualActiveTurnId.value,
        recovered,
        true,
      )
      runtimeId.value = recovered.runtimeId
      if (recovered.phase === 'idle') void consumeQueuedInput()
    }
  } catch (_) {
    // 快照恢复失败不覆盖时间线与现有错误；下一次运行事件仍可继续收敛。
  } finally {
    recoveringSnapshot = false
  }
}

async function forkAndSend(inputItems: RuntimeInputItem[]): Promise<boolean> {
  const source = reference.value
  const draft = engineDraft(props.session.id)
  const project = props.session.project_reference ?? draft?.project
  const cwd = props.session.cwd ?? draft?.cwd
  if (!source || !project || !cwd) {
    error.value = t('common.forkSessionFailed')
    return false
  }
  try {
    const created = await forkSession(source, null, {
      ...(selectedChannel.value ? { channelId: selectedChannel.value } : {}),
      ...(selectedModel.value ? { model: selectedModel.value } : {}),
    })
    const forkedSessionId = sessionUiId(created.session)
    inheritEngineRunConfig(props.session.id, forkedSessionId)
    registerEngineDraft(forkedSessionId, {
      reference: created.session,
      project,
      engineName: props.session.engine_name || enginePresentation.value.displayName,
      cwd,
      sourceMeta: created.sourceMeta,
      attachedChannel: selectedChannel.value,
      attachedCapabilityFingerprint: created.capabilityFingerprint,
    })
    await startTurnWithFastFallback(created.session, inputItems)
    input.value = ''
    return true
  } catch (cause) {
    error.value = causeMessage(cause)
    return false
  }
}

type SendPreparation = 'attached' | 'forked' | 'cancelled'

async function prepareSessionForSend(inputItems: RuntimeInputItem[]): Promise<SendPreparation> {
  resolvingWriterConflict.value = true
  error.value = null
  try {
    while (true) {
      const outcome = await ensureAttached()
      if (outcome === 'attached') return 'attached'
      if (outcome === 'failed') return 'cancelled'

      const choice = await confirmMulti(t('engine.codex.writerConflictMessage'), [
        {
          label: t('engine.codex.writerConflictRetry'),
          value: 'retry',
          style: 'success',
        },
        {
          label: t('engine.codex.writerConflictForkAndSend'),
          value: 'fork',
        },
      ])
      if (choice === 'retry') continue
      if (choice === 'fork') return await forkAndSend(inputItems) ? 'forked' : 'cancelled'
      return 'cancelled'
    }
  } finally {
    resolvingWriterConflict.value = false
  }
}

async function serializeRuntimeInput(
  text: string,
  images: readonly PendingImage[],
  skill?: { name: string; path: string },
): Promise<RuntimeInputItem[]> {
  const imageBlocks = images.length ? await imageInput.toImageBlocks(images) : []
  return [
    ...(text ? [{ kind: 'text' as const, text }] : []),
    ...(skill ? [{ kind: 'skill' as const, name: skill.name, path: skill.path }] : []),
    ...imageBlocks.map(block => ({
      kind: 'image' as const,
      mediaType: block.source.media_type,
      data: block.source.data,
    })),
  ]
}

function resetComposerField() {
  input.value = ''
  cursorPos.value = 0
  composerFieldRef.value?.resetHeight()
}

function syncCommandCursor() {
  cursorPos.value = textareaRef.value?.selectionStart ?? 0
}

function onCommandInput() {
  syncCommandCursor()
  commandError.value = null
}

function onCommandSelect(command: SlashCommand) {
  const prefix = composerPrefix(input.value) ?? command.wirePrefix
  if (command.hasArg) {
    const insertion = `${prefix}${command.name} `
    input.value = insertion
    nextTick(() => {
      const textarea = textareaRef.value
      if (!textarea) return
      textarea.focus()
      textarea.setSelectionRange(insertion.length, insertion.length)
      cursorPos.value = insertion.length
    })
    return
  }
  input.value = `${prefix}${command.name}`
  cursorPos.value = 0
  nextTick(() => { void send() })
}

function handleNativeCommand(command: SlashCommand) {
  switch (command.name) {
    case 'help':
      showHelpCard.value = true
      resumeTimelineFollow()
      break
    case 'clear':
      clearedRecordIds.value = new Set(
        composeRuntimeTimeline(records.value, liveRecords.value).map(record => record.id),
      )
      showHelpCard.value = false
      break
    case 'new':
      commandError.value = t('session.slashNewInWorkbench')
      break
    case 'cd':
      commandError.value = t('session.slashOpenInWorkbench')
      break
  }
}

function handleModelCommand(model: string): boolean {
  const normalized = model.toLocaleLowerCase()
  const descriptor = models.value.find(item =>
    item.model.toLocaleLowerCase() === normalized || item.id.toLocaleLowerCase() === normalized,
  )
  if (!descriptor) {
    commandError.value = t('slash.errorEngineModelUnknown')
    return false
  }
  onEngineModelChange(descriptor.model)
  return true
}

function selectedTurnOptions(serviceTier: string | null = selectedServiceTier.value) {
  return {
    ...(selectedModel.value ? { model: selectedModel.value } : {}),
    ...(selectedEffort.value ? { effort: selectedEffort.value } : {}),
    serviceTier,
  }
}

async function startTurnWithFastFallback(session: SessionRef, inputItems: RuntimeInputItem[]) {
  const requestedServiceTier = selectedServiceTier.value
  try {
    return await startTurnWithInput(session, inputItems, selectedTurnOptions(requestedServiceTier))
  } catch (cause) {
    if (!requestedServiceTier || !isFastServiceTierUnavailableError(cause)) throw cause
    // turn/start 被服务端拒绝时尚未创建 turn，可安全按标准档重放一次。
    selectedServiceTier.value = null
    fastModeNotice.value = t('topbar.fastModeFallback')
    return await startTurnWithInput(session, inputItems, selectedTurnOptions(null))
  }
}

async function submitRuntimeInput(item: QueuedRuntimeInput, restoreDraft: boolean): Promise<boolean> {
  if (!reference.value || sending.value) return false
  sending.value = true
  error.value = null
  const optimisticId = `pending-user-${Date.now()}`
  // startTurn 每次创建新 turn；不能继承可能滞后的运行时快照。
  liveRecords.value.push(createOptimisticUserRecord(
    reference.value,
    optimisticId,
    item.text,
    item.images,
  ))
  try {
    const turn = await startTurnWithFastFallback(reference.value, item.input)
    liveRecords.value = bindOptimisticUserTurn(
      liveRecords.value,
      optimisticId,
      turn.reference.nativeTurnId,
    )
    return true
  } catch (cause) {
    liveRecords.value = liveRecords.value.filter(record => record.id !== optimisticId)
    if (restoreDraft && item.text) {
      input.value = input.value.trim() ? `${item.text}\n${input.value}` : item.text
    }
    error.value = causeMessage(cause)
    return false
  } finally {
    sending.value = false
  }
}

async function consumeQueuedInput() {
  if (runtimeActive.value || sending.value || preparingInput.value || queuedInputs.value.length === 0) return
  const next = queuedInputs.value.shift()!
  const sent = await submitRuntimeInput(next, false)
  if (!sent) queuedInputs.value.unshift(next)
}

function removeQueuedInput(id: string) {
  queuedInputs.value = queuedInputs.value.filter(item => item.id !== id)
}

async function send() {
  let text = input.value.trim()
  const draftImages = [...imageInput.images.value]
  if ((!text && draftImages.length === 0) || !reference.value || preparingInput.value || sending.value || resolvingWriterConflict.value) return
  if (composerPrefix(text) && !composerCommandsReady.value) {
    await refreshComposerCommands()
  }
  const parsed = parseCommand(
    text,
    composerSkills.value,
    composerCommands.value,
    commandCatalogContext.value,
  )
  if (parsed.kind === 'invalid') {
    commandError.value = parsed.reason
    return
  }
  commandError.value = null
  if (parsed.kind === 'native') {
    handleNativeCommand(parsed.cmd)
    resetComposerField()
    return
  }
  if (parsed.kind === 'terminal') {
    commandError.value = t('slash.unsupportedForEngine')
    return
  }
  if (parsed.kind === 'pass' && parsed.cmd.name === 'model') {
    if (handleModelCommand(parsed.arg)) resetComposerField()
    return
  }
  let explicitSkill: { name: string; path: string } | undefined
  if (parsed.kind === 'pass') {
    text = formatCommandInvocation(parsed.cmd, parsed.arg)
    if (parsed.cmd.category === 'skill' && parsed.cmd.path && sessionEngineId.value === 'codex') {
      explicitSkill = { name: parsed.cmd.name, path: parsed.cmd.path }
    }
  }
  const queueForNextTurn = runtimeActive.value
  let consumeAfterPreparation = false
  preparingInput.value = true
  try {
    const inputItems = await serializeRuntimeInput(text, draftImages, explicitSkill)
    const queuedItem: QueuedRuntimeInput = {
      id: `queued-input-${++queuedInputSequence}`,
      text,
      imageCount: draftImages.length,
      images: draftImages.map(image => ({
        id: image.id,
        dataUrl: image.dataUrl,
        mediaType: image.mime,
      })),
      input: inputItems,
    }

    if (queueForNextTurn) {
      resetComposerField()
      imageInput.clearImages()
      queuedInputs.value.push(queuedItem)
      consumeAfterPreparation = !runtimeActive.value
      return
    }

    const preparation = await prepareSessionForSend(inputItems)
    if (preparation === 'forked') {
      resetComposerField()
      imageInput.clearImages()
      return
    }
    if (preparation !== 'attached' || !runtimeId.value) {
      if (preparation === 'attached') error.value = runtimeUnavailableReason.value
      return
    }

    resetComposerField()
    imageInput.clearImages()
    const sent = await submitRuntimeInput(queuedItem, true)
    if (!sent && draftImages.length) {
      imageInput.images.value = [...draftImages, ...imageInput.images.value]
    }
  } catch (cause) {
    error.value = causeMessage(cause)
  } finally {
    preparingInput.value = false
    if (consumeAfterPreparation) void consumeQueuedInput()
  }
}

async function interrupt() {
  if (interrupting.value || !reference.value || !runtimeId.value || !activeTurnId.value) return
  interrupting.value = true
  try {
    await interruptTurn(reference.value, runtimeId.value, activeTurnId.value)
  } catch (cause) {
    error.value = causeMessage(cause)
  } finally {
    interrupting.value = false
  }
}

function onEngineChannelChange(channelId: string | null) {
  fastModeNotice.value = null
  selectedChannel.value = channelId ?? configuredChannelId.value
  const channel = selectedChannel.value !== OFFICIAL_CHANNEL_ID
    ? channels.value.find(item => item.id === selectedChannel.value) ?? null
    : null
  const binding = engineChannelBinding(channel, sessionEngineId.value)
  const nextDefaultModel = engineDefaultModel.value
    ?? binding?.availableModels[0]
    ?? models.value.find(item => item.isDefault)?.model
    ?? null
  selectedModel.value = nextDefaultModel
  selectedEffort.value = engineDefaultEffort.value
    ?? models.value.find(item => item.model === nextDefaultModel)?.defaultEffort
    ?? null
  modelOverridden.value = false
  effortOverridden.value = false
}

function onEngineModelChange(model: string | null) {
  fastModeNotice.value = null
  if (model) {
    selectedModel.value = model
    modelOverridden.value = true
    return
  }
  modelOverridden.value = false
  selectedModel.value = engineDefaultModel.value
    ?? models.value.find(item => item.isDefault)?.model
    ?? models.value.find(item => !item.hidden)?.model
    ?? null
}

function onEngineEffortChange(effort: string | null) {
  effortOverridden.value = effort !== null
  selectedEffort.value = effort
    ?? engineDefaultEffort.value
    ?? models.value.find(item => item.model === selectedModel.value)?.defaultEffort
    ?? null
}

function onEngineFastModeChange(enabled: boolean) {
  fastModeNotice.value = null
  selectedServiceTier.value = enabled ? selectedFastTier.value?.id ?? null : null
}

async function decide(request: RuntimeSnapshot['pendingInteractions'][number], decision: string) {
  try {
    await respondInteraction(request.reference, decision)
  } catch (cause) {
    error.value = causeMessage(cause)
  }
}

function interactionSubject(request: InteractionRequest): string {
  if (request.title) return request.title
  if (request.kind === 'command') return t('engine.segment.command')
  if (request.kind === 'fileChange') return t('engine.segment.fileChange', { count: 1 })
  return t('engine.approvalRequired')
}

function interactionToolName(request: InteractionRequest): string {
  if (request.kind === 'command') return 'Bash'
  return `Approval:${request.kind}`
}

function interactionPayload(request: InteractionRequest): Record<string, unknown> {
  return request.payload && typeof request.payload === 'object' && !Array.isArray(request.payload)
    ? request.payload as Record<string, unknown>
    : { value: request.payload }
}

watch(pendingInteractions, (requests, previous = []) => {
  if (!interactive.value) return
  const nextIds = new Set(requests.map(request => request.reference.requestId))
  for (const request of previous) {
    if (!nextIds.has(request.reference.requestId)) clearHint(request.reference.requestId)
  }
  for (const request of requests) {
    requestHint(
      request.reference.requestId,
      interactionToolName(request),
      interactionPayload(request),
    )
  }
}, { immediate: true })

function interactionOptions(request: InteractionRequest): SessionApprovalOption[] {
  let safeOptionIndex = 0
  return request.options.map(option => {
    const tone = option.dangerous ? 'ghost' : safeOptionIndex++ === 0 ? 'primary' : 'warn'
    return {
      id: option.id,
      label: t(`engine.decision.${option.id}`, option.label),
      tone,
      icon: option.dangerous
        ? 'i-carbon-close'
        : tone === 'primary' ? 'i-carbon-checkmark' : 'i-carbon-time',
    }
  })
}

function interactionDefaultOption(request: InteractionRequest): string | null {
  return request.options.find(option => !option.dangerous)?.id ?? null
}

function interactionDenyOption(request: InteractionRequest): string | null {
  return request.options.find(option => option.id === 'decline' || option.id === 'deny')?.id
    ?? request.options.find(option => option.dangerous)?.id
    ?? null
}

async function refreshSessionActions() {
  const target = reference.value
  if (!target) return
  const generation = sessionGeneration
  try {
    const resolvedActions = await sessionActions(target)
    if (isCurrentTarget(target, generation)) actions.value = resolvedActions
  } catch (cause) {
    if (isCurrentTarget(target, generation)) error.value = causeMessage(cause)
  }
}

async function onGenerateSummary() {
  const target = reference.value
  if (!target || summaryGenerating.value) return
  summaryGenerating.value = true
  try {
    await refreshSummary(target, true)
  } catch (cause) {
    error.value = causeMessage(cause)
  } finally {
    summaryGenerating.value = false
  }
}

function scheduleTurnSettlement(turnId: string, attempt = 0) {
  const existing = settlementTimers.get(turnId)
  if (existing !== undefined) window.clearTimeout(existing)
  const generation = sessionGeneration
  const delay = TURN_SETTLEMENT_DELAYS[Math.min(attempt, TURN_SETTLEMENT_DELAYS.length - 1)]
  const timer = window.setTimeout(async () => {
    settlementTimers.delete(turnId)
    if (generation !== sessionGeneration || !completedTurnIds.has(turnId)) return
    await reload({ quiet: true })
    if (generation !== sessionGeneration || !hasLiveTurn(liveRecords.value, turnId)) {
      completedTurnIds.delete(turnId)
      if (generation === sessionGeneration) generateMetadataForSettledTurn(turnId)
      return
    }
    if (attempt + 1 < TURN_SETTLEMENT_DELAYS.length) {
      scheduleTurnSettlement(turnId, attempt + 1)
    }
  }, delay)
  settlementTimers.set(turnId, timer)
}

function applyRuntimeEvent(envelope: RuntimeEventEnvelope) {
  if (!ownsSession(envelope.session)) return
  if (envelope.event.kind === 'turnStarted') {
    bindLatestRuntimeOptimisticInput(envelope.session, envelope.event.turnId)
  }
  visualActiveTurnId.value = reduceRuntimeVisualActivity(
    visualActiveTurnId.value,
    envelope.event,
  )
  const effect = reduceRuntimeTimeline(liveRecords.value, envelope, {
    ...(selectedModel.value ? { model: selectedModel.value } : {}),
    ...(selectedEffort.value ? { effort: selectedEffort.value } : {}),
  })
  if (effect.changed) liveRecords.value = effect.records
  if (effect.error) error.value = effect.error
  if (effect.refreshActions) void refreshSessionActions()
  if (effect.completedTurnId) {
    completedTurnIds.add(effect.completedTurnId)
    scheduleTurnSettlement(effect.completedTurnId)
    void consumeQueuedInput()
  }
}

const runtimeDeltaShaper = useRuntimeDeltaShaper(applyRuntimeEvent)
const typingActive = computed(() => sending.value || visualActiveTurnId.value !== null)

function isTurnStreaming(turnId: string | null): boolean {
  return !!turnId && turnId === visualActiveTurnId.value
}

function onInputKeydown(event: KeyboardEvent) {
  if (shouldSubmitComposer(event)) {
    event.preventDefault()
    void send()
  }
}

watch(() => props.session.id, async () => {
  sessionGeneration++
  const generation = sessionGeneration
  const sessionId = props.session.id
  if (pendingDraftReplacement && pendingDraftReplacement.sessionId !== sessionId) {
    pendingDraftReplacement = null
  }
  const replacement = pendingDraftReplacement?.sessionId === sessionId
    ? pendingDraftReplacement
    : null
  lastAppliedTimelineRequestId = 0
  cancelTurnSettlements()
  runtimeDeltaShaper.reset()
  resetTimelineFollow()
  toolFoldState.reset()
  runConfigSyncing.value = true
  try {
    resetSessionBanner()
    fastModeNotice.value = null
    records.value = []
    liveRecords.value = []
    clearedRecordIds.value = new Set()
    showHelpCard.value = false
    commandError.value = null
    cursorPos.value = 0
    queuedInputs.value = []
    imageInput.clearImages()
    snapshot.value = null
    visualActiveTurnId.value = null
    runtimeId.value = replacement?.runtimeId ?? null
    if (replacement) {
      runtimeProviderId.value = engineProviderIdFromSource(replacement.sourceMeta, sessionEngineId.value)
      attachedChannel.value = replacement.attachedChannel
      attachedCapabilityFingerprint.value = replacement.attachedCapabilityFingerprint
    } else {
      models.value = []
      selectedModel.value = null
      selectedEffort.value = null
      selectedServiceTier.value = null
      modelOverridden.value = false
      effortOverridden.value = false
      selectedChannel.value = null
      attachedChannel.value = undefined
      runtimeProviderId.value = null
      attachedCapabilityFingerprint.value = undefined
    }
    asyncPanelOpen.value = false
    menuOpen.value = false
    await reload()
    await nextTick()
    requestTimelineFollow(true)
    if (interactive.value && !replacement) await loadRuntimeConfiguration()
  } finally {
    if (generation !== sessionGeneration || props.session.id !== sessionId) return
    runConfigSyncing.value = false
    setEngineRunConfig(sessionId, {
      model: selectedModel.value,
      effort: selectedEffort.value,
      serviceTier: selectedServiceTier.value,
      channelId: selectedChannel.value,
      modelOverridden: modelOverridden.value,
      effortOverridden: effortOverridden.value,
    })
    if (pendingDraftReplacement?.sessionId === sessionId) pendingDraftReplacement = null
  }
})

watch(() => asyncTasks.value.length, length => {
  if (length === 0) asyncPanelOpen.value = false
})

watch(() => allRecords.value.length, () => {
  requestTimelineFollow()
})

watch(timelineContentElement, element => {
  timelineResizeObserver?.disconnect()
  if (!element) return
  if (!timelineResizeObserver) {
    // itemDelta 会原地增长同一条记录，数组 length 不变；以真实内容高度统一触发跟随。
    timelineResizeObserver = new ResizeObserver(() => requestTimelineFollow())
  }
  timelineResizeObserver.observe(element)
})

watch(selectedModel, (model) => {
  const descriptor = models.value.find(item => item.model === model)
  if (!descriptor?.efforts.some(item => item.id === selectedEffort.value)) {
    selectedEffort.value = descriptor?.defaultEffort ?? null
    effortOverridden.value = false
  }
  if (!descriptor?.serviceTiers.some(tier => tier.id === selectedServiceTier.value)) {
    selectedServiceTier.value = null
  }
})

watch(effectiveChannel, channel => {
  if (channel) selectedServiceTier.value = null
})

watch([selectedModel, selectedEffort, selectedServiceTier, selectedChannel, modelOverridden, effortOverridden], ([model, effort, serviceTier, channelId, modelIsOverridden, effortIsOverridden]) => {
  if (runConfigSyncing.value) return
  setEngineRunConfig(props.session.id, {
    model,
    effort,
    serviceTier,
    channelId,
    modelOverridden: modelIsOverridden,
    effortOverridden: effortIsOverridden,
  })
})

onMounted(async () => {
  imageInput.attach()
  unlistenSnapshot = await listen<RuntimeSnapshot>('engine-runtime-snapshot', event => {
    if (ownsSession(event.payload.session)) {
      snapshot.value = event.payload
      runtimeId.value = event.payload.runtimeId
      visualActiveTurnId.value = syncRuntimeVisualActivity(
        visualActiveTurnId.value,
        event.payload,
      )
      if (!event.payload.sequenceConsistent) void recoverRuntimeSnapshot()
      if (event.payload.phase === 'idle' && visualActiveTurnId.value === null) {
        void consumeQueuedInput()
      }
    }
  })
  unlistenEvent = await listen<RuntimeEventEnvelope[]>('engine-runtime-events', event => {
    for (const envelope of event.payload) runtimeDeltaShaper.push(envelope)
  })
  unlistenSourceChange = await listen<SourceChangeEnvelope>('engine-source-change', event => {
    const { instance, change } = event.payload
    if (!reference.value || !sameInstance(instance, reference.value.engine)) return
    if (change.kind === 'fullRefresh' || (change.session && ownsSession(change.session))) {
      void reload({ quiet: true })
    }
  })
  await Promise.all([reload(), recoverRuntimeSnapshot(), refreshChannels()])
  if (interactive.value) await loadRuntimeConfiguration()
})

onUnmounted(() => {
  sessionGeneration++
  resetSessionBanner()
  cancelTurnSettlements()
  invalidateTimelineScrollRequests()
  clearTimelineUpwardSettleTimer()
  timelineResizeObserver?.disconnect()
  timelineResizeObserver = null
  lastGroupResizeObserver?.disconnect()
  lastGroupResizeObserver = null
  stickySurfaceResizeObserver?.disconnect()
  stickySurfaceResizeObserver = null
  if (stickyUpdateFrame) window.cancelAnimationFrame(stickyUpdateFrame)
  stickyUpdateFrame = 0
  unlistenSnapshot?.()
  unlistenEvent?.()
  unlistenSourceChange?.()
  if (interactive.value) {
    for (const request of pendingInteractions.value) clearHint(request.reference.requestId)
  }
})
</script>

<template>
  <SessionSurface :root-ref="bindDetailRoot">
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
            :fast-mode-notice="fastModeNotice"
            :narrow="containerWidth < 280"
            @model-change="onEngineModelChange"
            @effort-change="onEngineEffortChange"
            @fast-mode-change="onEngineFastModeChange"
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

    <SessionViewport :scroll-ref="bindViewport" @wheel="onTimelineWheel" @scroll="onTimelineScroll">
      <template #overlay>
        <SessionBannerOverlay
          :visible="interactive && sessionBannerVisible"
          :session-id="nativeSessionId"
          :resumed="sessionBannerResumed"
          :cwd="session.cwd || ''"
          :model="selectedModel"
          :effort="selectedEffort"
          :features="htmlVisualEnabled ? [t('settings.htmlVisual')] : []"
          :hook-events="[]"
        />
      </template>
      <div
        v-if="stickyUserPromptEnabled"
        ref="stickyOverlayElement"
        class="engine-sticky-user-overlay"
      >
        <div
          v-if="stickyDisplay"
          ref="stickySurfaceElement"
          class="engine-sticky-user-surface"
          :style="{ transform: `translate3d(0, ${stickyPushOffset}px, 0)` }"
          :title="t('session.stickyJumpHint')"
          @click="scrollToConversationGroup(stickyDisplay.index)"
        >
          <ConversationUserMessage :time-label="stickyTimeLabel">
            <EngineSegmentBlock
              v-for="(segment, index) in stickyUserSegments"
              :key="index"
              :segment="segment"
            />
            <template #actions>
              <span class="flex items-center gap-0.5">
                <button
                  type="button"
                  class="engine-sticky-nav-btn"
                  :disabled="stickyPreviousIndex < 0"
                  :title="t('session.stickyPrev')"
                  @click.stop="scrollToConversationGroup(stickyPreviousIndex)"
                ><span class="i-carbon-chevron-up h-3.5 w-3.5" /></button>
                <button
                  type="button"
                  class="engine-sticky-nav-btn"
                  :disabled="stickyNextIndex < 0"
                  :title="t('session.stickyNext')"
                  @click.stop="scrollToConversationGroup(stickyNextIndex)"
                ><span class="i-carbon-chevron-down h-3.5 w-3.5" /></button>
              </span>
            </template>
          </ConversationUserMessage>
        </div>
      </div>
      <SessionContentState v-if="loading && !records.length">{{ t('common.loading') }}</SessionContentState>
      <SessionContentState v-else-if="!allRecords.length && !showHelpCard">{{ t('session.noRecords') }}</SessionContentState>
      <div v-else ref="timelineContentElement" class="pb-2 relative">
        <div
          v-if="shouldVirtualize"
          ref="historicalVirtualBoxElement"
          :style="{ height: `${conversationVirtualizer.getTotalSize()}px`, position: 'relative', width: '100%' }"
        >
          <div
            v-for="virtualItem in conversationVirtualizer.getVirtualItems()"
            :key="String(virtualItem.key)"
            :ref="element => element && conversationVirtualizer.measureElement(element as Element)"
            :data-index="virtualItem.index"
            :data-conversation-index="virtualItem.index"
            :class="{ 'sticky-source-hidden': stickyDisplay?.index === virtualItem.index }"
            :style="{
              position: 'absolute',
              top: 0,
              left: 0,
              width: '100%',
              transform: `translateY(${virtualItem.start}px)`,
            }"
          >
            <EngineConversationGroup
              :records="historicalGroups[virtualItem.index].records"
              :engine-id="sessionEngineId"
              :engine-name="enginePresentation.displayName"
              :model="timelineModel"
              :accent="engineAccent"
              :show-thought-process="enginePresentation.showThoughtProcess"
              :day-label="historicalGroups[virtualItem.index].dayLabel"
              :streaming="isTurnStreaming(historicalGroups[virtualItem.index].turnId)"
              :artifact-root="session.cwd"
            />
          </div>
        </div>
        <div v-else class="space-y-4">
          <div
            v-for="(group, index) in historicalGroups"
            :key="`${session.id}:${group.key}`"
            :data-conversation-index="index"
            :class="{ 'sticky-source-hidden': stickyDisplay?.index === index }"
          >
            <EngineConversationGroup
              :records="group.records"
              :engine-id="sessionEngineId"
              :engine-name="enginePresentation.displayName"
              :model="timelineModel"
              :accent="engineAccent"
              :show-thought-process="enginePresentation.showThoughtProcess"
              :day-label="group.dayLabel"
              :streaming="isTurnStreaming(group.turnId)"
              :artifact-root="session.cwd"
            />
          </div>
        </div>
        <div
          v-if="lastConversationGroup"
          :key="`${session.id}:${lastConversationGroup.key}`"
          :data-group-key="`${session.id}:${lastConversationGroup.key}`"
          :data-conversation-index="conversationGroups.length - 1"
          :class="[
            historicalGroups.length > 0 ? 'mt-4' : '',
            { 'sticky-source-hidden': stickyDisplay?.index === conversationGroups.length - 1 },
          ]"
          ref="lastGroupElement"
        >
          <EngineConversationGroup
            :records="lastConversationGroup.records"
            :engine-id="sessionEngineId"
            :engine-name="enginePresentation.displayName"
            :model="timelineModel"
            :accent="engineAccent"
            :show-thought-process="enginePresentation.showThoughtProcess"
            :day-label="lastConversationGroup.dayLabel"
            :streaming="isTurnStreaming(lastConversationGroup.turnId)"
            :artifact-root="session.cwd"
            :auto-open-artifact="!isTurnStreaming(lastConversationGroup.turnId)"
          />
        </div>
        <SlashHelpCard v-if="showHelpCard" :commands="allComposerCommands" />
        <SessionTypingIndicator :active="typingActive" />
      </div>
      <SessionContentState v-if="error" tone="danger">{{ error }}</SessionContentState>
      <SessionBackToBottom v-if="!followTimeline" @click="resumeTimelineFollow" />
    </SessionViewport>

    <template #interaction>
      <SessionInteractionPanel v-if="interactive && pendingInteractions.length">
        <SessionApprovalCard
          v-for="request in pendingInteractions"
          :key="request.reference.requestId"
          :title="t('permission.title')"
          :subject="interactionSubject(request)"
          :options="interactionOptions(request)"
          :default-option-id="interactionDefaultOption(request)"
          :deny-option-id="interactionDenyOption(request)"
          :keyboard="pendingInteractions.length === 1"
          @decide="decide(request, $event)"
        >
          <template #hint>
            <div
              v-if="getHint(request.reference.requestId)?.text || getHint(request.reference.requestId)?.loading"
              class="mx-3 mt-1.5 flex items-start gap-1.5 rounded border border-border/60 bg-muted/40 px-2 py-1.5"
            >
              <span class="i-carbon-sparkle mt-px h-3.5 w-3.5 shrink-0 text-primary/60" aria-hidden="true" />
              <span v-if="getHint(request.reference.requestId)?.loading" class="text-[10px] italic text-muted-foreground">{{ t('permission.analyzing') }}</span>
              <span v-else class="text-[10px] leading-relaxed text-foreground/80">{{ getHint(request.reference.requestId)?.text }}</span>
            </div>
          </template>
          <component
            :is="resolveTool(interactionToolName(request))"
            :input="request.payload"
            :tool-use-id="request.reference.requestId"
            :name="interactionToolName(request)"
          />
        </SessionApprovalCard>
      </SessionInteractionPanel>
    </template>

    <template #input>
      <SessionComposer
        v-if="canSend && !hideInput"
        :dragging="imageInput.isDragging.value"
        :busy="isBusy"
        :has-content="!!input.trim() || !!imageInput.images.value.length"
        :can-send-while-busy="canSendWhileBusy"
        :send-disabled="attaching || preparingInput || sending || resolvingWriterConflict"
        :stop-disabled="interrupting || !activeTurnId || actions?.interrupt.available === false"
        :stop-loading="interrupting"
        @send="send"
        @stop="interrupt"
      >
        <template #notices>
          <div v-if="commandError" class="mb-1 text-xs text-destructive">
            {{ commandError }}
          </div>
        </template>

        <template #overlay>
          <SlashCommandPanel
            :visible="commandPanelVisible"
            :query="input"
            :skills="composerSkills"
            :commands="composerCommands"
            :context="commandCatalogContext"
            class="absolute bottom-full left-4 mb-1"
            @select="onCommandSelect"
            @close="commandError = null"
          />
        </template>

        <template #queue>
          <SessionComposerQueue :items="composerQueueItems" @remove="removeQueuedInput" />
        </template>

        <template #attachments>
          <SessionComposerAttachments
            :images="imageInput.images.value"
            :dragging="imageInput.isDragging.value"
            :error="imageInput.lastError.value?.message"
            @remove="imageInput.removeImage"
          />
        </template>

        <template #field="{ fieldClass }">
          <SessionComposerField
            ref="composerFieldRef"
            v-model="input"
            :class="fieldClass"
            :placeholder="t('session.inputPlaceholder')"
            :disabled="attaching || resolvingWriterConflict"
            @keydown="onInputKeydown"
            @input="onCommandInput"
            @keyup="syncCommandCursor"
            @click="syncCommandCursor"
            @select="syncCommandCursor"
          />
        </template>
      </SessionComposer>
      <div v-else-if="interactive && !hideInput" class="shrink-0 border-t border-border bg-card px-3 py-2 text-center text-xs text-muted-foreground">{{ runtimeUnavailableReason }}</div>
    </template>

    <template #footer>
      <SessionReadonlyBar v-if="mode === 'archive'" :label="t('session.readonlyPreview')">
        <button
          type="button"
          class="shrink-0 rounded border border-border px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-45"
          :disabled="summaryGenerating"
          @click="onGenerateSummary"
        >
          <span v-if="summaryGenerating" class="i-carbon-renew mr-1 h-3 w-3 animate-spin" />
          <span v-else class="i-carbon-text-short-paragraph mr-1 h-3 w-3" />
          {{ currentSummary ? t('archive.refreshSummary') : t('archive.generateSummary') }}
        </button>
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
      <SessionSidePanel
        :mounted="sidePanelDom"
        :expanded="sidePanelExpanded"
        :width="sidePanelWidth"
      >
        <EngineAsyncTaskPanel
          v-if="asyncPanelVisible && reference"
          :session="reference"
          :engine-name="enginePresentation.displayName"
          :tasks="asyncTasks"
          :accent="engineAccent"
          @close="asyncPanelOpen = false"
        />
      </SessionSidePanel>
    </template>
  </SessionSurface>
</template>

<style scoped>
.engine-sticky-user-overlay {
  position: sticky;
  top: 0;
  z-index: 20;
  height: 0;
  overflow: visible;
  pointer-events: none;
}
.engine-sticky-user-surface {
  cursor: pointer;
  pointer-events: auto;
  will-change: transform;
}
.sticky-source-hidden :deep(.conversation-user-message) { visibility: hidden; }
.engine-sticky-nav-btn {
  display: inline-flex;
  padding: 1px;
  border: 0;
  border-radius: var(--radius);
  color: var(--muted-foreground);
  background: transparent;
  cursor: pointer;
}
.engine-sticky-nav-btn:hover:not(:disabled) {
  color: var(--foreground);
  background: var(--muted);
}
.engine-sticky-nav-btn:disabled {
  cursor: default;
  opacity: 0.3;
}
</style>

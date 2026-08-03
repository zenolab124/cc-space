<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useI18n } from 'vue-i18n'
import type { SessionSummary } from '@/types'
import type { ConversationRecord, EngineSegment, ModelDescriptor, RuntimeEventEnvelope, RuntimeSnapshot, SessionActions } from '@/engines/types'
import { attachSession, interruptTurn, listModels, loadTimeline, respondInteraction, runtimeSnapshots, sessionActions, startTurn, steerTurn } from '@/engines/client'
import { sameInstance, sessionKey } from '@/engines/identity'
import EngineSegmentBlock from './EngineSegmentBlock.vue'
import { useSessionMeta } from '@/composables/useSessionMeta'
import { useWorkbench } from '@/composables/useWorkbench'
import { useUiState } from '@/composables/useUiState'
import { useProjects } from '@/composables/useProjects'
import { useSessions } from '@/composables/useSessions'
import { useConfirm } from '@/composables/useConfirm'

const props = withDefaults(defineProps<{
  session: SessionSummary
  mode?: 'archive' | 'workbench'
  hideInput?: boolean
}>(), { mode: 'archive', hideInput: false })

const { t } = useI18n()
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
const { getMeta, updateMeta } = useSessionMeta()
const { openSession, removeSession, findSession } = useWorkbench()
const { switchSection } = useUiState()
const { loadProjects } = useProjects()
const { selectSession } = useSessions()
const { confirm } = useConfirm()
let unlistenSnapshot: UnlistenFn | null = null
let unlistenEvent: UnlistenFn | null = null
let recoveringSnapshot = false

const reference = computed(() => props.session.reference)
const allRecords = computed(() => [...records.value, ...liveRecords.value])
const interactive = computed(() => props.mode === 'workbench' && !!reference.value)
const canSend = computed(() => interactive.value && actions.value?.send.available === true)
const runtimeUnavailableReason = computed(() => {
  const reason = actions.value?.send.reasonCode ?? actions.value?.resume.reasonCode
  return reason ? t(reason, t('engine.runtimeUnavailable')) : t('engine.runtimeUnavailable')
})
const isBusy = computed(() => snapshot.value?.phase === 'running' || snapshot.value?.phase === 'awaitingInteraction' || sending.value)
const activeTurnId = computed(() => snapshot.value?.activeTurnId ?? null)
const pendingInteractions = computed(() => snapshot.value?.pendingInteractions ?? [])
const effortOptions = computed(() => models.value.find(model => model.model === selectedModel.value)?.efforts ?? [])
const starred = computed(() => !!getMeta(props.session.id)?.starred)
const resolvedTitle = computed(() => getMeta(props.session.id)?.title || props.session.title || props.session.first_user_message || props.session.native_id)
const tags = computed(() => getMeta(props.session.id)?.tags ?? [])
const resumeUnavailableReason = computed(() => {
  const reason = actions.value?.resume.reasonCode
  return reason ? t(reason, t('engine.runtimeUnavailable')) : t('engine.runtimeUnavailable')
})

async function toggleStar() {
  await updateMeta(props.session.id, { starred: !starred.value }, reference.value)
}

function beginEditMeta() {
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
  const approved = await confirm(t('engine.softDeleteConfirm'), t('common.delete'))
  if (!approved) return
  await updateMeta(props.session.id, { deleted: true, deletedAt: new Date().toISOString() }, reference.value)
  if (findSession(props.session.id)) removeSession(props.session.id)
  selectSession(null)
  await loadProjects()
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
  if (!reference.value || runtimeId.value || attaching.value || actions.value?.resume.available !== true) return
  attaching.value = true
  try {
    const attached = await attachSession(reference.value)
    runtimeId.value = attached.runtimeId
    try {
      models.value = await listModels(reference.value.engine)
      const defaultModel = models.value.find(model => model.isDefault) ?? models.value.find(model => !model.hidden)
      selectedModel.value = defaultModel?.model ?? null
      selectedEffort.value = defaultModel?.defaultEffort ?? null
    } catch (_) {
      models.value = []
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
  await ensureAttached()
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
        sourceMeta: {},
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

function roleLabel(role: ConversationRecord['role']) {
  return t(`engine.role.${role}`)
}

function roleClass(role: ConversationRecord['role']) {
  return role === 'user' ? 'ml-10 bg-primary/8' : 'mr-10 bg-card'
}

function onInputKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault()
    void send()
  }
}

watch(() => props.session.id, async () => {
  records.value = []
  liveRecords.value = []
  snapshot.value = null
  runtimeId.value = null
  await reload()
  if (interactive.value) await ensureAttached()
})

watch(selectedModel, (model) => {
  selectedEffort.value = models.value.find(item => item.model === model)?.defaultEffort ?? null
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
  await Promise.all([reload(), recoverRuntimeSnapshot()])
  if (interactive.value) await ensureAttached()
})

onUnmounted(() => {
  unlistenSnapshot?.()
  unlistenEvent?.()
})
</script>

<template>
  <div class="h-full min-h-0 flex flex-col bg-background">
    <header class="shrink-0 flex items-center gap-2 border-b border-border bg-card px-3 py-2">
      <span class="px-1.5 py-0.5 rounded bg-secondary text-[10px] text-muted-foreground">{{ session.engine_name }}</span>
      <div class="min-w-0 flex-1 truncate text-xs font-semibold">{{ resolvedTitle }}</div>
      <span v-for="tag in tags.slice(0, 2)" :key="tag" class="max-w-24 truncate rounded bg-secondary px-1.5 py-0.5 text-[9px] text-muted-foreground">{{ tag }}</span>
      <span v-if="snapshot" class="text-[10px] text-muted-foreground">{{ t(`engine.phase.${snapshot.phase}`) }}</span>
      <button type="button" class="icon-btn icon-btn-sm" :aria-label="t('engine.editMetadata')" :title="t('engine.editMetadata')" @click="beginEditMeta">
        <span class="i-carbon-edit h-3.5 w-3.5" />
      </button>
      <button type="button" class="icon-btn icon-btn-sm" :aria-label="t('engine.star')" :title="t('engine.star')" @click="toggleStar">
        <span class="h-3.5 w-3.5" :class="starred ? 'i-carbon-star-filled text-primary' : 'i-carbon-star'" />
      </button>
      <button v-if="actions?.openCwd.available" type="button" class="icon-btn icon-btn-sm" :aria-label="t('engine.openCwd')" :title="t('engine.openCwd')" @click="openCwd">
        <span class="i-carbon-folder h-3.5 w-3.5" />
      </button>
      <button v-if="mode === 'archive'" type="button" class="rounded border border-border px-2 py-1 text-[10px] hover:bg-muted disabled:cursor-not-allowed disabled:opacity-45" :disabled="actions?.resume.available !== true" :title="actions?.resume.available === true ? t('asyncTask.openInWorkbench') : resumeUnavailableReason" @click="openInWorkbench">{{ t('asyncTask.openInWorkbench') }}</button>
      <button v-if="mode === 'archive'" type="button" class="icon-btn icon-btn-sm text-destructive" :aria-label="t('common.delete')" :title="t('common.delete')" @click="softDelete">
        <span class="i-carbon-trash-can h-3.5 w-3.5" />
      </button>
      <select v-if="interactive && models.length" v-model="selectedModel" class="form-select max-w-44 text-xs" :aria-label="t('engine.model')">
        <option v-for="model in models.filter(item => !item.hidden)" :key="model.id" :value="model.model">{{ model.displayName }}</option>
      </select>
      <select v-if="interactive && effortOptions.length" v-model="selectedEffort" class="form-select max-w-32 text-xs" :aria-label="t('engine.effort')">
        <option v-for="effort in effortOptions" :key="effort.id" :value="effort.id">{{ effort.id }}</option>
      </select>
    </header>

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

    <div class="flex-1 min-h-0 overflow-y-auto p-3">
      <div v-if="loading && !records.length" class="py-10 text-center text-xs text-muted-foreground">{{ t('common.loading') }}</div>
      <div v-else-if="!allRecords.length" class="py-10 text-center text-xs text-muted-foreground">{{ t('session.noRecords') }}</div>
      <div v-else class="mx-auto max-w-220 space-y-2">
        <article v-for="record in allRecords" :key="`${record.id}:${record.timestamp}`" class="rounded border border-border p-3 shadow-paper" :class="roleClass(record.role)">
          <div class="mb-1 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">{{ roleLabel(record.role) }}</div>
          <EngineSegmentBlock v-for="(segment, index) in record.segments" :key="index" :segment="segment" />
        </article>
      </div>
      <p v-if="error" role="alert" class="mx-auto mt-3 max-w-220 rounded border border-destructive/30 bg-destructive/5 p-2 text-xs text-destructive">{{ error }}</p>
    </div>

    <section v-if="interactive && pendingInteractions.length" class="shrink-0 border-t border-border bg-card px-3 py-2" aria-live="polite">
      <div v-for="request in pendingInteractions" :key="request.reference.requestId" class="mx-auto max-w-220 rounded border border-border bg-muted/40 p-2">
        <div class="text-xs font-medium">{{ request.title || t('engine.approvalRequired') }}</div>
        <pre class="mt-1 max-h-32 overflow-auto whitespace-pre-wrap text-[10px] text-muted-foreground">{{ JSON.stringify(request.payload, null, 2) }}</pre>
        <div class="mt-2 flex flex-wrap gap-1.5">
          <button v-for="option in request.options" :key="option.id" type="button" class="rounded border border-border px-2 py-1 text-xs hover:bg-muted" :class="option.dangerous ? 'text-destructive' : 'text-foreground'" @click="decide(request, option.id)">
            {{ t(`engine.decision.${option.id}`, option.label) }}
          </button>
        </div>
      </div>
    </section>

    <form v-if="canSend && !hideInput" class="shrink-0 border-t border-border bg-card p-2" @submit.prevent="send">
      <div class="mx-auto flex max-w-220 items-end gap-2">
        <textarea v-model="input" rows="2" class="min-h-14 flex-1 resize-none rounded border border-input bg-background px-2.5 py-2 text-sm outline-none focus:border-ring" :placeholder="t('engine.inputPlaceholder')" :disabled="attaching" @keydown="onInputKeydown" />
        <button v-if="isBusy && activeTurnId && actions?.steer.available" type="submit" class="rounded bg-primary px-3 py-2 text-xs text-primary-foreground disabled:opacity-50" :disabled="!input.trim() || sending">{{ t('engine.steer') }}</button>
        <button v-if="isBusy && activeTurnId && actions?.interrupt.available !== false" type="button" class="rounded border border-border px-3 py-2 text-xs text-destructive hover:bg-muted" @click="interrupt">{{ t('engine.interrupt') }}</button>
        <button v-if="!isBusy" type="submit" class="rounded bg-primary px-3 py-2 text-xs text-primary-foreground disabled:opacity-50" :disabled="!input.trim() || attaching || sending">{{ t('engine.send') }}</button>
      </div>
    </form>
    <div v-else-if="interactive && !hideInput" class="shrink-0 border-t border-border bg-card px-3 py-2 text-center text-xs text-muted-foreground">{{ runtimeUnavailableReason }}</div>
    <div v-else-if="mode === 'archive'" class="shrink-0 border-t border-border bg-card px-3 py-2 text-center text-xs text-muted-foreground">{{ t('session.readonlyPreview') }}</div>
  </div>
</template>

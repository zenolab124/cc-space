<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ConversationRecord, SessionRef } from '@/engines/types'
import type { EngineAsyncTask, EngineAsyncTaskState } from '@/engines/asyncTasks'
import type { EngineAccent } from '@/engines/presentation'
import { loadTimeline } from '@/engines/client'
import EngineConversationGroup from './EngineConversationGroup.vue'
import { groupConversationRecords } from '@/engines/conversationGroups'

const props = defineProps<{
  session: SessionRef
  engineName: string
  tasks: EngineAsyncTask[]
  accent?: EngineAccent
}>()

const emit = defineEmits<{ (event: 'close'): void }>()
const { t, locale } = useI18n()
const selectedThreadId = ref<string | null>(null)
const selected = computed(() => props.tasks.find(task => task.threadId === selectedThreadId.value) ?? null)
const records = ref<ConversationRecord[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const accentColor = computed(() => `var(--${props.accent ?? 'primary'})`)

const groups = computed(() => groupConversationRecords(records.value))

const stateClass: Record<EngineAsyncTaskState, string> = {
  running: '',
  completed: 'bg-muted text-muted-foreground',
  failed: 'bg-destructive/10 text-destructive',
  unknown: 'bg-accent/10 text-accent',
}

function stateStyle(state: EngineAsyncTaskState): Record<string, string> | undefined {
  if (state !== 'running') return undefined
  return {
    color: accentColor.value,
    background: `color-mix(in srgb, ${accentColor.value} 12%, transparent)`,
  }
}

function shortThreadId(value: string): string {
  return value.slice(0, 8)
}

function formatTimestamp(value: string | null): string {
  if (!value) return ''
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleString(locale.value)
}

async function openTask(task: EngineAsyncTask) {
  selectedThreadId.value = task.threadId
  records.value = []
  error.value = null
  loading.value = true
  try {
    const timeline = await loadTimeline({
      engine: props.session.engine,
      nativeId: task.threadId,
    })
    records.value = timeline.records
  } catch (cause) {
    error.value = String(cause)
  } finally {
    loading.value = false
  }
}

function backToList() {
  selectedThreadId.value = null
  records.value = []
  error.value = null
}

watch(() => props.session.nativeId, backToList)
watch(() => props.tasks.map(task => task.key).join('|'), () => {
  if (selected.value && !props.tasks.some(task => task.key === selected.value?.key)) backToList()
})
watch(
  () => selected.value ? `${selected.value.threadId}:${selected.value.updatedAt ?? ''}:${selected.value.state}` : '',
  (next, previous) => {
    if (next && previous && next !== previous && selected.value) void openTask(selected.value)
  },
)
</script>

<template>
  <aside class="h-full min-h-0 flex flex-col bg-card">
    <header class="h-10 shrink-0 flex items-center gap-2 border-b border-border px-3">
      <button
        v-if="selected"
        type="button"
        class="icon-btn icon-btn-sm"
        :aria-label="t('common.back')"
        :title="t('common.back')"
        @click="backToList"
      >
        <span class="i-carbon-chevron-left h-3.5 w-3.5" />
      </button>
      <span v-else class="i-carbon-lightning h-3.5 w-3.5 shrink-0" :style="{ color: accentColor }" />
      <div class="min-w-0 flex-1 truncate text-xs font-semibold">
        {{ selected?.title || t('engine.async.title') }}
      </div>
      <span
        v-if="selected"
        class="shrink-0 rounded-full px-1.5 py-0.5 text-[9px] font-semibold"
        :class="stateClass[selected.state]"
        :style="stateStyle(selected.state)"
      >{{ t(`asyncTask.state.${selected.state}`) }}</span>
      <span
        v-else
        class="rounded-full px-1.5 py-0.5 text-[10px] font-semibold tabular-nums"
        :style="stateStyle('running')"
      >
        {{ tasks.length }}
      </span>
      <button
        v-if="selected"
        type="button"
        class="icon-btn icon-btn-sm"
        :disabled="loading"
        :aria-label="t('common.refresh')"
        :title="t('common.refresh')"
        @click="openTask(selected)"
      >
        <span class="i-carbon-renew h-3 w-3" :class="loading && 'animate-spin'" />
      </button>
      <button type="button" class="icon-btn icon-btn-sm" :aria-label="t('common.close')" :title="t('common.close')" @click="emit('close')">
        <span class="i-carbon-close h-3 w-3" />
      </button>
    </header>

    <div v-if="!selected" class="flex-1 min-h-0 overflow-y-auto p-2.5 space-y-1.5">
      <button
        v-for="task in tasks"
        :key="task.key"
        type="button"
        class="w-full rounded border border-border bg-background px-2.5 py-2 text-left shadow-paper transition-colors hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        @click="openTask(task)"
      >
        <div class="flex min-w-0 items-center gap-2">
          <span class="i-carbon-bot h-3.5 w-3.5 shrink-0" :style="{ color: accentColor }" />
          <span class="min-w-0 flex-1 truncate text-xs font-medium">{{ task.title }}</span>
          <span class="rounded-full px-1.5 py-0.5 text-[9px] font-semibold" :class="stateClass[task.state]" :style="stateStyle(task.state)">
            {{ t(`asyncTask.state.${task.state}`) }}
          </span>
        </div>
        <div class="mt-1 flex min-w-0 items-center gap-1.5 text-[10px] text-muted-foreground">
          <span class="font-mono">{{ shortThreadId(task.threadId) }}</span>
          <span v-if="task.model">· {{ task.model }}</span>
          <span v-if="task.effort">· {{ task.effort }}</span>
          <span v-if="formatTimestamp(task.updatedAt)" class="ml-auto shrink-0">{{ formatTimestamp(task.updatedAt) }}</span>
        </div>
        <p v-if="task.message" class="mt-1 line-clamp-2 text-[10px] text-muted-foreground">{{ task.message }}</p>
      </button>
    </div>

    <div v-else class="flex-1 min-h-0 overflow-y-auto px-3 py-3 overscroll-contain">
      <div class="mb-3 rounded border border-border bg-muted/25 p-2.5 text-[11px]">
        <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-muted-foreground">
          <span class="font-mono">{{ shortThreadId(selected.threadId) }}</span>
          <span v-if="selected.model">{{ selected.model }}</span>
          <span v-if="selected.effort">{{ selected.effort }}</span>
        </div>
        <p v-if="selected.prompt" class="mt-1.5 whitespace-pre-wrap text-foreground/80">{{ selected.prompt }}</p>
      </div>
      <div v-if="loading" class="py-8 text-center text-xs text-muted-foreground">{{ t('common.loading') }}</div>
      <p v-else-if="error" role="alert" class="rounded border border-destructive/30 bg-destructive/5 p-2 text-xs text-destructive">{{ error }}</p>
      <div v-else-if="groups.length" class="space-y-4 pb-2">
        <EngineConversationGroup
          v-for="group in groups"
          :key="group.key"
          :records="group.records"
          :engine-name="engineName"
          :model="selected.model"
          :accent="accent ?? 'primary'"
        />
      </div>
      <div v-else class="py-8 text-center text-xs text-muted-foreground">{{ t('engine.async.noTranscript') }}</div>
    </div>
  </aside>
</template>

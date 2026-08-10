<script setup lang="ts">
import { computed, inject, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  engineProcessActivityState,
  type EngineProcessActivity,
} from '@/engines/processGroups'
import {
  TOOL_FOLD_INTERACTION,
  useToolFoldState,
} from '@/composables/useToolDisplay'
import SessionProcessDisclosure from '@/components/session/SessionProcessDisclosure.vue'

const props = withDefaults(defineProps<{
  activity: EngineProcessActivity
  active?: boolean
  mode?: 'card' | 'individual'
}>(), {
  active: false,
  mode: 'card',
})

const { t } = useI18n()
const foldState = useToolFoldState()
const onInteraction = inject(TOOL_FOLD_INTERACTION, () => {})
const contentId = `engine-process-item-${useId()}`
const state = computed(() => engineProcessActivityState(props.activity, props.active))
const expanded = computed(() => {
  if (foldState.collapsedItems.has(props.activity.id)) return false
  return foldState.expandedItems.has(props.activity.id)
    || foldState.itemDefaultExpanded.value
})

const iconClass = computed(() => {
  if (props.activity.kind === 'command') return 'i-carbon-terminal'
  if (props.activity.kind === 'fileChange') return 'i-carbon-document-blank'
  const name = props.activity.call?.name.toLowerCase() ?? ''
  if (name.includes('read')) return 'i-carbon-document-view'
  if (name.includes('search') || name.includes('find')) return 'i-carbon-search'
  return 'i-carbon-tool-kit'
})

const kindLabel = computed(() => {
  if (props.activity.kind === 'command') return t('engine.segment.command')
  if (props.activity.kind === 'fileChange') return t('engine.segment.fileChangeKind')
  return props.activity.call ? t('engine.segment.toolCall') : t('engine.segment.toolResult')
})

const title = computed(() => {
  if (props.activity.kind === 'command') {
    return props.activity.segment.command.replace(/\s+/g, ' ').trim()
      || t('engine.segment.command')
  }
  if (props.activity.kind === 'fileChange') {
    return t('engine.segment.fileChange', { count: props.activity.segment.changes.length })
  }
  return props.activity.call?.name || t('engine.segment.toolResult')
})

const stateLabel = computed(() => {
  if (state.value === 'running') return t('block.toolFold.running')
  if (state.value === 'error') return t('block.toolFold.failed')
  if (state.value === 'interrupted') return t('block.toolFold.interrupted')
  if (state.value === 'done') return t('block.toolFold.done')
  return ''
})

function prettyValue(value: unknown): string {
  if (typeof value === 'string') return value
  try {
    return JSON.stringify(value, null, 2)
  } catch (_) {
    return String(value)
  }
}

function toggle(event: MouseEvent) {
  onInteraction()
  if (event.shiftKey) {
    foldState.setAllItems(!expanded.value)
    return
  }
  if (expanded.value) {
    foldState.expandedItems.delete(props.activity.id)
    foldState.collapsedItems.add(props.activity.id)
  } else {
    foldState.collapsedItems.delete(props.activity.id)
    foldState.expandedItems.add(props.activity.id)
  }
}
</script>

<template>
  <div
    v-if="mode === 'card'"
    class="engine-activity-card"
    :data-tool-use-id="activity.id"
  >
    <div class="engine-activity-header">
      <span :class="iconClass" class="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      <span class="shrink-0 text-[10px] text-muted-foreground">{{ kindLabel }}</span>
      <span class="min-w-0 flex-1 truncate font-medium text-foreground">{{ title }}</span>
      <span v-if="stateLabel" class="engine-activity-state" :class="`is-${state}`">
        <span v-if="state === 'running'" class="i-carbon-renew h-3 w-3 animate-spin" />
        <span v-else-if="state === 'done'" class="i-carbon-checkmark h-3 w-3" />
        <span v-else-if="state === 'error'" class="i-carbon-warning-alt h-3 w-3" />
        <span v-else class="i-carbon-stop-outline h-3 w-3" />
        {{ stateLabel }}
      </span>
    </div>

    <template v-if="activity.kind === 'command'">
      <div v-if="activity.segment.cwd" class="engine-activity-cwd">{{ activity.segment.cwd }}</div>
      <pre v-if="activity.segment.command" class="engine-activity-code">{{ activity.segment.command }}</pre>
      <pre v-if="activity.segment.output" class="engine-activity-result">{{ activity.segment.output }}</pre>
    </template>

    <template v-else-if="activity.kind === 'fileChange'">
      <div
        v-for="change in activity.segment.changes"
        :key="`${change.kind}:${change.path}`"
        class="engine-activity-change"
      >
        <div class="flex min-w-0 items-center gap-1.5">
          <span class="rounded bg-muted px-1 py-0.5 text-[9px] uppercase text-muted-foreground">{{ change.kind }}</span>
          <span class="min-w-0 truncate font-mono text-[10.5px]">{{ change.path }}</span>
        </div>
        <pre v-if="change.diff" class="engine-activity-result">{{ change.diff }}</pre>
      </div>
    </template>

    <template v-else>
      <pre v-if="activity.call" class="engine-activity-code">{{ prettyValue(activity.call.input) }}</pre>
      <pre
        v-if="activity.result"
        class="engine-activity-result"
        :class="activity.result.isError && 'text-destructive'"
      >{{ prettyValue(activity.result.content) }}</pre>
    </template>
  </div>

  <SessionProcessDisclosure
    v-else
    :expanded="expanded"
    :state="state"
    :content-id="contentId"
    @toggle="toggle"
  >
    <template #summary>
      <span :class="iconClass" class="h-3.5 w-3.5 shrink-0" />
      <span class="shrink-0 text-muted-foreground/65">{{ kindLabel }}</span>
      <span class="min-w-0 truncate text-foreground/80">{{ title }}</span>
    </template>
    <template #status>
      <span v-if="stateLabel" class="engine-activity-state" :class="`is-${state}`">
        {{ stateLabel }}
      </span>
    </template>
    <EngineProcessItem :activity="activity" :active="active" mode="card" />
  </SessionProcessDisclosure>
</template>

<style scoped>
.engine-activity-card {
  min-width: 0;
  padding: 7px 9px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--background);
  font-size: 11px;
}
.engine-activity-header {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
  min-height: var(--tool-row-line-height);
}
.engine-activity-state {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 3px;
  color: var(--muted-foreground);
  font-size: 10.5px;
}
.engine-activity-state.is-running { color: var(--primary); }
.engine-activity-state.is-error,
.engine-activity-state.is-interrupted { color: var(--destructive); }
.engine-activity-cwd {
  margin-top: 5px;
  overflow: hidden;
  color: var(--muted-foreground);
  font-family: var(--font-mono);
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.engine-activity-code,
.engine-activity-result {
  margin-top: 6px;
  overflow-x: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-family: var(--font-mono);
  font-size: 10.5px;
  line-height: 1.55;
}
.engine-activity-code {
  padding: 6px 8px;
  border-radius: calc(var(--radius) - 1px);
  background: var(--muted);
}
.engine-activity-result {
  padding-top: 6px;
  border-top: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
  color: var(--muted-foreground);
}
.engine-activity-change {
  margin-top: 6px;
}
.engine-activity-change + .engine-activity-change {
  padding-top: 7px;
  border-top: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
}
</style>

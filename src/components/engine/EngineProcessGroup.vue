<script setup lang="ts">
import { computed, inject, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  buildEngineProcessActivities,
  engineProcessActivityState,
  type EngineProcessActivity,
  type EngineResponseSegmentEntry,
} from '@/engines/processGroups'
import {
  TOOL_FOLD_INTERACTION,
  useToolDisplayMode,
  useToolFoldState,
} from '@/composables/useToolDisplay'
import SessionProcessDisclosure from '@/components/session/SessionProcessDisclosure.vue'
import EngineProcessItem from './EngineProcessItem.vue'

const props = withDefaults(defineProps<{
  entries: EngineResponseSegmentEntry[]
  active?: boolean
}>(), {
  active: false,
})

const { t } = useI18n()
const { toolDisplayMode } = useToolDisplayMode()
const foldState = useToolFoldState()
const onInteraction = inject(TOOL_FOLD_INTERACTION, () => {})
const contentId = `engine-process-${useId()}`
const activities = computed(() => buildEngineProcessActivities(props.entries))
const states = computed(() => activities.value.map(activity =>
  engineProcessActivityState(activity, props.active),
))
const groupState = computed(() => {
  for (const state of ['error', 'interrupted', 'running', 'unknown', 'done'] as const) {
    if (states.value.includes(state)) return state
  }
  return 'unknown'
})
const failedCount = computed(() => states.value.filter(state => state === 'error').length)
const groupKey = computed(() => activities.value[0]?.id ?? props.entries[0]?.key ?? '')
const containsRequested = computed(() => activities.value.some(activity =>
  activity.id === foldState.requestedToolId.value,
))
const expanded = computed(() => {
  if (containsRequested.value) return true
  if (foldState.collapsedGroups.has(groupKey.value)) return false
  return foldState.expandedGroups.has(groupKey.value)
    || foldState.groupDefaultExpanded.value
})

function activityLabel(activity: EngineProcessActivity): string {
  if (activity.kind === 'command') {
    return activity.segment.command.replace(/\s+/g, ' ').trim()
      || t('engine.segment.command')
  }
  if (activity.kind === 'fileChange') {
    return t('engine.segment.fileChange', { count: activity.segment.changes.length })
  }
  return activity.call?.name || t('engine.segment.toolResult')
}

const visibleSummary = computed(() => activities.value.slice(0, 2))
const hiddenActivityCount = computed(() => Math.max(0, activities.value.length - visibleSummary.value.length))
const summaryTitle = computed(() => [
  activities.value.map(activityLabel).join(' · '),
  t('block.foldShiftHint'),
].filter(Boolean).join('\n'))
const stateLabel = computed(() => {
  if (groupState.value === 'error') {
    return t('block.toolFold.failedCount', { count: failedCount.value })
  }
  if (groupState.value === 'interrupted') return t('block.toolFold.interrupted')
  if (groupState.value === 'running') {
    const current = Math.max(1, states.value.findIndex(state => state === 'running') + 1)
    return t('block.toolFold.runningProgress', { current, total: activities.value.length })
  }
  return ''
})

function toggle(event: MouseEvent) {
  onInteraction()
  if (event.shiftKey) {
    foldState.setAllGroups(!expanded.value)
    return
  }
  if (expanded.value) {
    foldState.expandedGroups.delete(groupKey.value)
    foldState.collapsedGroups.add(groupKey.value)
  } else {
    foldState.collapsedGroups.delete(groupKey.value)
    foldState.expandedGroups.add(groupKey.value)
  }
}
</script>

<template>
  <div v-if="toolDisplayMode === 'cards'" class="engine-process-cards">
    <EngineProcessItem
      v-for="activity in activities"
      :key="activity.key"
      :activity="activity"
      :active="active"
      mode="card"
    />
  </div>

  <div v-else-if="toolDisplayMode === 'individual'" class="engine-process-items">
    <EngineProcessItem
      v-for="activity in activities"
      :key="activity.key"
      :activity="activity"
      :active="active"
      mode="individual"
    />
  </div>

  <SessionProcessDisclosure
    v-else
    :expanded="expanded"
    :state="groupState"
    :title="summaryTitle"
    :content-id="contentId"
    @toggle="toggle"
  >
    <template #summary>
      <span class="engine-process-summary">
        <template v-for="(activity, index) in visibleSummary" :key="activity.key">
          <span v-if="index" class="engine-process-separator" aria-hidden="true">·</span>
          <span>{{ activityLabel(activity) }}</span>
        </template>
        <span v-if="hiddenActivityCount" class="engine-process-more">+{{ hiddenActivityCount }}</span>
      </span>
    </template>
    <template #status>
      <span v-if="stateLabel" class="engine-process-state" :class="`is-${groupState}`" aria-live="polite">
        {{ stateLabel }}
        <span v-if="groupState === 'running'" class="engine-process-dots" aria-hidden="true"><i /><i /><i /></span>
      </span>
    </template>

    <div class="engine-process-cards">
      <EngineProcessItem
        v-for="activity in activities"
        :key="activity.key"
        :activity="activity"
        :active="active"
        mode="card"
      />
    </div>
  </SessionProcessDisclosure>
</template>

<style scoped>
.engine-process-cards,
.engine-process-items {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: var(--message-block-gap);
}
.engine-process-summary {
  display: flex;
  min-width: 0;
  align-items: baseline;
  gap: 4px;
  overflow: hidden;
  color: var(--foreground);
  font-size: 10.5px;
  font-weight: 450;
  line-height: var(--tool-row-line-height);
  white-space: nowrap;
}
.engine-process-summary > span:not(.engine-process-separator, .engine-process-more) {
  overflow: hidden;
  text-overflow: ellipsis;
}
.engine-process-separator,
.engine-process-more {
  flex: none;
  color: color-mix(in srgb, currentColor 65%, transparent);
}
.engine-process-state {
  display: inline-flex;
  flex: none;
  align-items: center;
  color: var(--muted-foreground);
  font-size: 11px;
  line-height: var(--tool-row-line-height);
}
.engine-process-state.is-running { color: var(--primary); }
.engine-process-state.is-error,
.engine-process-state.is-interrupted { color: var(--destructive); }
.engine-process-dots {
  display: inline-flex;
  width: 17px;
  gap: 2px;
  margin-left: 4px;
}
.engine-process-dots i {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.22;
  animation: engine-process-dot-wave 1.05s infinite ease-in-out;
}
.engine-process-dots i:nth-child(2) { animation-delay: 150ms; }
.engine-process-dots i:nth-child(3) { animation-delay: 300ms; }
@keyframes engine-process-dot-wave {
  0%, 60%, 100% { opacity: 0.22; transform: translateY(0); }
  30% { opacity: 1; transform: translateY(-2px); }
}
@media (prefers-reduced-motion: reduce) {
  .engine-process-dots i { animation: none; opacity: 0.65; }
}
</style>

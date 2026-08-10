<script setup lang="ts">
import { computed, provide } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ConversationRecord, EngineSegment } from '@/engines/types'
import type { ConversationTurnView, EngineAccent } from '@/engines/presentation'
import type { AssistantResponseMeta } from '@/utils/assistantResponse'
import { engineResponseMeta } from '@/engines/presentation'
import {
  buildEngineResponseBlocks,
  isEngineThoughtSegment,
  projectEngineProcessEntries,
  type EngineProcessProjection,
  type EngineResponseBlock,
} from '@/engines/processGroups'
import type { ToolResultData } from '@/utils/toolPair'
import {
  TOOL_EXECUTION_CONTEXT,
  type ToolVisualState,
} from '@/composables/useToolDisplay'
import ConversationTurn from '@/components/session/ConversationTurn.vue'
import ContentBlockList from '@/components/ContentBlockList.vue'
import EngineSegmentBlock from './EngineSegmentBlock.vue'

const props = defineProps<{
  records: ConversationRecord[]
  engineName: string
  model: string | null
  accent?: EngineAccent
  showThoughtProcess?: boolean
  dayLabel?: string | null
  streaming?: boolean
}>()

const { locale } = useI18n()

function isRenderable(segment: EngineSegment): boolean {
  if (props.showThoughtProcess === false && isEngineThoughtSegment(segment)) return false
  if (segment.kind === 'unknown') return !!segment.summary?.trim()
  if (segment.kind === 'reasoning') {
    return segment.visibility === 'redacted' || !!segment.text.trim()
  }
  if (segment.kind === 'text') return !!segment.text.trim()
  return true
}

const userSegments = computed(() => props.records
  .filter(record => record.role === 'user')
  .flatMap(record => record.segments)
  .filter(isRenderable))

const responseRecords = computed(() => props.records
  .filter(record => record.role !== 'user')
  .map(record => ({
    ...record,
    segments: record.segments.filter(isRenderable),
  }))
  .filter(record => record.segments.length > 0))

const responseTimeline = computed(() => props.records.filter(record => record.role !== 'user'))
type ResponseBlockView =
  | Extract<EngineResponseBlock, { kind: 'content' }>
  | (Extract<EngineResponseBlock, { kind: 'process' }> & { projection: EngineProcessProjection })

const responseBlocks = computed<ResponseBlockView[]>(() => buildEngineResponseBlocks(
  responseRecords.value,
).map(block => block.kind === 'process'
  ? { ...block, projection: projectEngineProcessEntries(block.entries) }
  : block))

const toolResults = computed(() => {
  const results = new Map<string, ToolResultData>()
  for (const block of responseBlocks.value) {
    if (block.kind !== 'process') continue
    for (const [id, result] of block.projection.results) results.set(id, result)
  }
  return results
})

const toolVisualStates = computed(() => {
  const states = new Map<string, ToolVisualState>()
  for (const block of responseBlocks.value) {
    if (block.kind !== 'process') continue
    for (const [id, state] of block.projection.states) states.set(id, state)
  }
  return states
})

provide('toolResultMap', toolResults)
provide(TOOL_EXECUTION_CONTEXT, {
  results: toolResults,
  visualStates: toolVisualStates,
})

function dateTimeLabel(value: string | null | undefined): string {
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

function timeLabelOf(value: string | null | undefined): string {
  if (!value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  return date.toLocaleTimeString(locale.value, { hour: '2-digit', minute: '2-digit' })
}

const timeLabel = computed(() => {
  const value = props.records.find(record => record.role === 'user')?.timestamp
    ?? props.records[0]?.timestamp
  return dateTimeLabel(value)
})

const completedAt = computed(() => [...responseTimeline.value]
  .reverse()
  .find(record => record.timestamp)?.timestamp ?? null)

const engineMeta = computed(() => engineResponseMeta(props.records, props.model))

const responseMeta = computed<AssistantResponseMeta>(() => ({
  ...engineMeta.value,
  completedText: timeLabelOf(completedAt.value),
  completedFull: dateTimeLabel(completedAt.value),
}))
const turnView = computed<ConversationTurnView>(() => ({
  dayLabel: props.dayLabel ?? null,
  timeLabel: timeLabel.value || null,
  user: {
    visible: userSegments.value.length > 0,
    sticky: responseRecords.value.length > 0,
    hidden: false,
  },
  response: {
    visible: responseRecords.value.length > 0,
    meta: responseMeta.value,
    showFooter: props.streaming !== true,
    speaker: props.engineName,
    accent: props.accent ?? 'primary',
  },
  lazy: true,
}))
</script>

<template>
  <ConversationTurn :turn="turnView">
    <template #user>
      <EngineSegmentBlock v-for="(segment, index) in userSegments" :key="index" :segment="segment" />
    </template>
    <template #response>
      <template v-for="block in responseBlocks" :key="block.key">
        <ContentBlockList
          v-if="block.kind === 'process'"
          :blocks="block.projection.blocks"
          :streaming="streaming"
        />
        <EngineSegmentBlock v-else :segment="block.entry.segment" :streaming="streaming" />
      </template>
    </template>
  </ConversationTurn>
</template>

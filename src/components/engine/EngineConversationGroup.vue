<script setup lang="ts">
import { computed, provide } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ConversationRecord, EngineSegment } from '@/engines/types'
import type { ConversationTurnView, EngineAccent } from '@/engines/presentation'
import type { AssistantResponseMeta } from '@/utils/assistantResponse'
import { engineResponseMeta } from '@/engines/presentation'
import {
  buildEngineResponseBlocks,
  isEngineProcessSegment,
  isRenderableEngineSegment,
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
import ArtifactPreviewList from '@/components/artifacts/ArtifactPreviewList.vue'
import { detectEngineSegmentArtifacts } from '@/features/artifact-preview/detectArtifacts'

const props = defineProps<{
  records: ConversationRecord[]
  engineName: string
  model: string | null
  accent?: EngineAccent
  showThoughtProcess?: boolean
  dayLabel?: string | null
  streaming?: boolean
  artifactRoot?: string | null
}>()

const { locale } = useI18n()

function isRenderable(segment: EngineSegment): boolean {
  return isRenderableEngineSegment(segment, props.showThoughtProcess !== false)
}

const userSegments = computed(() => props.records
  .filter(record => record.role === 'user')
  .flatMap(record => record.segments)
  .filter(isRenderable))
const optimisticImages = computed(() => props.records
  .filter(record => record.role === 'user')
  .flatMap(record => Array.isArray(record.sourceMeta.optimisticImages)
    ? record.sourceMeta.optimisticImages
    : [])
  .filter((image): image is { id: string; dataUrl: string; mediaType: string } => {
    if (!image || typeof image !== 'object') return false
    const candidate = image as Record<string, unknown>
    return typeof candidate.id === 'string'
      && typeof candidate.dataUrl === 'string'
      && typeof candidate.mediaType === 'string'
  }))

const responseRecords = computed(() => props.records
  .filter(record => record.role !== 'user')
  .map(record => ({
    ...record,
    segments: record.segments.filter(isRenderable),
  }))
  .filter(record => record.segments.length > 0))

const responseTimeline = computed(() => props.records.filter(record => record.role !== 'user'))
const artifactCandidates = computed(() => props.streaming
  ? []
  : detectEngineSegmentArtifacts(responseTimeline.value.flatMap(record => record.segments)))
const responseProcessEntries = computed(() => responseRecords.value.flatMap(record => record.segments
  .map((segment, index) => ({ key: `${record.id}:${index}`, segment }))
  .filter(entry => isEngineProcessSegment(entry.segment))))
type ResponseBlockView =
  | Extract<EngineResponseBlock, { kind: 'content' }>
  | (Extract<EngineResponseBlock, { kind: 'process' }> & { projection: EngineProcessProjection })

const responseBlocks = computed<ResponseBlockView[]>(() => buildEngineResponseBlocks(
  responseRecords.value,
).map(block => block.kind === 'process'
  ? { ...block, projection: projectEngineProcessEntries(block.entries, responseProcessEntries.value) }
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
    visible: userSegments.value.length > 0 || optimisticImages.value.length > 0,
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
      <div v-if="optimisticImages.length" class="mt-2 flex flex-wrap gap-2">
        <img
          v-for="image in optimisticImages"
          :key="image.id"
          :src="image.dataUrl"
          :alt="image.mediaType"
          class="max-h-48 max-w-full rounded border border-border object-contain shadow-paper"
        />
      </div>
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
      <ArtifactPreviewList
        v-if="artifactRoot"
        :candidates="artifactCandidates"
        :root="artifactRoot"
      />
    </template>
  </ConversationTurn>
</template>

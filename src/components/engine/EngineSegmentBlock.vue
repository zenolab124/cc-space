<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ContentBlock } from '@/types'
import { resolveAsset } from '@/engines/client'
import type { EngineSegment } from '@/engines/types'
import BlockText from '@/components/blocks/BlockText.vue'
import BlockThinking from '@/components/blocks/BlockThinking.vue'
import SessionProcessDisclosure from '@/components/session/SessionProcessDisclosure.vue'

const props = defineProps<{ segment: EngineSegment }>()
const { t } = useI18n()
const expanded = ref(false)
const assetUrl = ref<string | null>(null)
const assetError = ref<string | null>(null)
const loadingAsset = ref(false)

const textBlock = computed<Extract<ContentBlock, { type: 'text' }> | null>(() =>
  props.segment.kind === 'text' ? { type: 'text', text: props.segment.text } : null,
)

const thinkingBlock = computed<Extract<ContentBlock, { type: 'thinking' }> | null>(() => {
  if (props.segment.kind !== 'reasoning') return null
  return {
    type: 'thinking',
    thinking: props.segment.visibility === 'redacted' ? '' : props.segment.text,
    ...(props.segment.visibility === 'redacted' ? { signature: 'redacted' } : {}),
  }
})

const isDisclosure = computed(() => [
  'commandExecution',
  'fileChange',
  'toolCall',
  'toolResult',
].includes(props.segment.kind))

const disclosureLabel = computed(() => {
  const segment = props.segment
  if (segment.kind === 'commandExecution') {
    const command = segment.command.replace(/\s+/g, ' ').trim()
    return command || t('engine.segment.command')
  }
  if (segment.kind === 'fileChange') {
    return t('engine.segment.fileChange', { count: segment.changes.length })
  }
  if (segment.kind === 'toolCall') return segment.name || t('engine.segment.toolCall')
  if (segment.kind === 'toolResult') return t('engine.segment.toolResult')
  return ''
})

const disclosureKind = computed(() => {
  if (props.segment.kind === 'commandExecution') return t('engine.segment.command')
  if (props.segment.kind === 'fileChange') return t('engine.segment.fileChangeKind')
  if (props.segment.kind === 'toolCall') return t('engine.segment.toolCall')
  if (props.segment.kind === 'toolResult') return t('engine.segment.toolResult')
  return ''
})

const status = computed(() => {
  const segment = props.segment
  return segment.kind === 'commandExecution' || segment.kind === 'fileChange' ? segment.status : null
})

const statusLabel = computed(() => status.value ? t(`engine.itemStatus.${status.value}`, status.value) : '')
const hasUnknownSummary = computed(() => props.segment.kind === 'unknown' && !!props.segment.summary?.trim())

function prettyValue(value: unknown): string {
  if (typeof value === 'string') return value
  try {
    return JSON.stringify(value, null, 2)
  } catch (_) {
    return String(value)
  }
}

async function loadAsset() {
  if (props.segment.kind !== 'attachment' || assetUrl.value || loadingAsset.value) return
  loadingAsset.value = true
  try {
    const result = await resolveAsset(props.segment.asset.session, props.segment.asset.nativeId)
    assetUrl.value = URL.createObjectURL(new Blob([new Uint8Array(result.bytes)], { type: result.mediaType }))
  } catch (error) {
    assetError.value = String(error)
  } finally {
    loadingAsset.value = false
  }
}

onUnmounted(() => {
  if (assetUrl.value) URL.revokeObjectURL(assetUrl.value)
})
</script>

<template>
  <BlockText v-if="textBlock" :block="textBlock" />

  <BlockThinking v-else-if="thinkingBlock" :block="thinkingBlock" />

  <div v-else-if="segment.kind === 'attachment'" class="my-1.5">
    <img
      v-if="assetUrl"
      :src="assetUrl"
      :alt="segment.title || t('engine.attachment')"
      class="max-h-96 max-w-full rounded border border-border shadow-paper"
    />
    <button
      v-else
      type="button"
      class="inline-flex items-center gap-1 text-xs text-primary hover:underline disabled:opacity-50"
      :disabled="loadingAsset"
      @click="loadAsset"
    >
      <span :class="loadingAsset ? 'i-carbon-renew animate-spin' : 'i-carbon-image'" class="h-3 w-3" />
      {{ loadingAsset ? t('common.loading') : (segment.title || t('engine.loadAttachment')) }}
    </button>
    <p v-if="assetError" role="alert" class="mt-1 text-xs text-destructive">{{ assetError }}</p>
  </div>

  <SessionProcessDisclosure
    v-else-if="isDisclosure"
    :expanded="expanded"
    :state="status || 'neutral'"
    @toggle="expanded = !expanded"
  >
    <template #summary>
      <span
        class="h-3 w-3 shrink-0"
        :class="segment.kind === 'commandExecution'
          ? 'i-carbon-terminal'
          : segment.kind === 'fileChange'
            ? 'i-carbon-document-blank'
            : 'i-carbon-tool-box'"
      />
      <span class="shrink-0 text-muted-foreground/65">{{ disclosureKind }}</span>
      <span class="min-w-0 truncate text-foreground/80">{{ disclosureLabel }}</span>
    </template>
    <template #status>
      <span v-if="status" class="engine-process-status" :class="`is-${status}`">
        <span v-if="status === 'running'" class="i-carbon-renew h-3 w-3 animate-spin" />
        <span v-else-if="status === 'completed'" class="i-carbon-checkmark h-3 w-3" />
        <span v-else-if="status === 'failed'" class="i-carbon-warning-alt h-3 w-3" />
        <span v-else-if="status === 'interrupted' || status === 'declined'" class="i-carbon-stop-outline h-3 w-3" />
        <span v-else class="i-carbon-time h-3 w-3" />
        {{ statusLabel }}
      </span>
    </template>

      <template v-if="segment.kind === 'commandExecution'">
        <div v-if="segment.cwd" class="mb-1 truncate text-[10px] text-muted-foreground">{{ segment.cwd }}</div>
        <pre v-if="segment.command" class="engine-code">{{ segment.command }}</pre>
        <pre v-if="segment.output" class="engine-output">{{ segment.output }}</pre>
      </template>

      <template v-else-if="segment.kind === 'fileChange'">
        <div v-for="change in segment.changes" :key="`${change.kind}:${change.path}`" class="engine-change">
          <div class="flex min-w-0 items-center gap-1.5 text-[11px]">
            <span class="rounded bg-muted px-1 py-0.5 text-[9px] uppercase text-muted-foreground">{{ change.kind }}</span>
            <span class="min-w-0 truncate">{{ change.path }}</span>
          </div>
          <pre v-if="change.diff" class="engine-output mt-1">{{ change.diff }}</pre>
        </div>
      </template>

      <pre v-else-if="segment.kind === 'toolCall'" class="engine-code">{{ prettyValue(segment.input) }}</pre>
      <pre v-else-if="segment.kind === 'toolResult'" class="engine-output" :class="segment.isError && 'text-destructive'">{{ prettyValue(segment.content) }}</pre>
  </SessionProcessDisclosure>

  <div v-else-if="hasUnknownSummary && segment.kind === 'unknown'" class="my-1 flex items-start gap-1.5 text-[11px] text-muted-foreground">
    <span class="i-carbon-information h-3 w-3 shrink-0 translate-y-0.5" />
    <span>{{ segment.summary }}</span>
  </div>
</template>

<style scoped>
.engine-process-status {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 3px;
  color: var(--muted-foreground);
}
.engine-process-status.is-running { color: var(--primary); }
.engine-process-status.is-failed,
.engine-process-status.is-declined,
.engine-process-status.is-interrupted { color: var(--destructive); }
.engine-code,
.engine-output {
  overflow-x: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-family: var(--font-mono);
  font-size: 10.5px;
  line-height: 1.55;
}
.engine-code {
  padding: 6px 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--muted);
}
.engine-output {
  margin-top: 5px;
  color: var(--muted-foreground);
}
.engine-change + .engine-change {
  margin-top: 7px;
  padding-top: 7px;
  border-top: 1px solid var(--border);
}
</style>

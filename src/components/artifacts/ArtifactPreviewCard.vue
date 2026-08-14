<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { formatBytes } from '@/types'
import {
  artifactFileName,
  type ArtifactCandidate,
  type ArtifactKind,
} from '@/features/artifact-preview/detectArtifacts'
import {
  ARTIFACT_SIZE_MESSAGE,
  MIN_ARTIFACT_FRAME_HEIGHT,
  clampArtifactFrameHeight,
  prepareSandboxedHtml,
} from '@/features/artifact-preview/sandboxHtml'

interface LoadedArtifact {
  fileName: string
  kind: 'html' | 'svg' | 'image'
  mediaType: string
  sizeBytes: number
  text: string | null
  data: string | null
}

const props = defineProps<{
  candidate: ArtifactCandidate
  root: string
  autoOpen?: boolean
}>()

const { t } = useI18n()
const cardRef = ref<HTMLElement | null>(null)
const stageRef = ref<HTMLElement | null>(null)
const frameRef = ref<HTMLIFrameElement | null>(null)
const artifact = ref<LoadedArtifact | null>(null)
const loading = ref(false)
const expanded = ref(false)
const error = ref<string | null>(null)
const sandboxNonce = ref('')
const frameHeight = ref(MIN_ARTIFACT_FRAME_HEIGHT)
let visibilityObserver: IntersectionObserver | null = null
let stageResizeObserver: ResizeObserver | null = null
let disposeTimer: number | null = null
let heightFrame = 0
let measuredContentHeight = Number.POSITIVE_INFINITY
let pendingContentHeight = Number.POSITIVE_INFINITY
let contentFillsViewport = true
let pendingFillsViewport = true
let loadRevision = 0
let autoOpenAttempted = false
let inPreviewRange = true

const fileName = computed(() => artifact.value?.fileName || artifactFileName(props.candidate.path))
const imageSource = computed(() => {
  const value = artifact.value
  return value?.data ? `data:${value.mediaType};base64,${value.data}` : null
})
const kindLabel = computed(() => {
  const labels: Record<ArtifactKind, string> = {
    html: 'HTML',
    svg: 'SVG',
    gif: 'GIF',
    image: t('artifactPreview.image'),
  }
  return labels[props.candidate.kind]
})

const sandboxedHtml = computed(() => {
  const source = artifact.value?.text
  if (!source || !sandboxNonce.value) return ''
  return prepareSandboxedHtml(source, sandboxNonce.value)
})

function resetFrameMeasurement() {
  measuredContentHeight = Number.POSITIVE_INFINITY
  pendingContentHeight = Number.POSITIVE_INFINITY
  contentFillsViewport = true
  pendingFillsViewport = true
  const width = stageRef.value?.clientWidth ?? cardRef.value?.clientWidth ?? 0
  frameHeight.value = width > 0
    ? clampArtifactFrameHeight(Number.POSITIVE_INFINITY, width)
    : MIN_ARTIFACT_FRAME_HEIGHT
}

async function loadPreview() {
  if (loading.value) return
  const revision = ++loadRevision
  loading.value = true
  error.value = null
  try {
    const loaded = await invoke<LoadedArtifact>('read_artifact_preview', {
      root: props.root,
      path: props.candidate.path,
    })
    if (revision !== loadRevision) return
    sandboxNonce.value = loaded.kind === 'html' ? crypto.randomUUID() : ''
    resetFrameMeasurement()
    artifact.value = loaded
    expanded.value = true
  } catch (cause) {
    if (revision !== loadRevision) return
    error.value = String(cause)
  } finally {
    if (revision === loadRevision) loading.value = false
  }
}

function togglePreview() {
  if (!artifact.value) {
    void loadPreview()
    return
  }
  expanded.value = !expanded.value
}

function reloadPreview() {
  loadRevision++
  artifact.value = null
  sandboxNonce.value = ''
  void loadPreview()
}

function clearDisposeTimer() {
  if (disposeTimer === null) return
  window.clearTimeout(disposeTimer)
  disposeTimer = null
}

function disposePreview() {
  clearDisposeTimer()
  loadRevision++
  loading.value = false
  expanded.value = false
  artifact.value = null
  sandboxNonce.value = ''
  resetFrameMeasurement()
}

function scheduleDispose() {
  clearDisposeTimer()
  disposeTimer = window.setTimeout(() => {
    disposeTimer = null
    if (!inPreviewRange && (artifact.value || loading.value)) disposePreview()
  }, 500)
}

function maybeAutoOpen() {
  if (!props.autoOpen || autoOpenAttempted || !inPreviewRange) return
  autoOpenAttempted = true
  void loadPreview()
}

function updateFrameHeight() {
  const width = stageRef.value?.clientWidth ?? 0
  if (width <= 0) return
  const contentHeight = contentFillsViewport ? Number.POSITIVE_INFINITY : measuredContentHeight
  const nextHeight = clampArtifactFrameHeight(contentHeight, width)
  if (Math.abs(nextHeight - frameHeight.value) >= 2) frameHeight.value = nextHeight
}

function scheduleFrameHeight(contentHeight: number, fillsViewport: boolean) {
  pendingContentHeight = contentHeight
  pendingFillsViewport = fillsViewport
  if (heightFrame) return
  heightFrame = window.requestAnimationFrame(() => {
    heightFrame = 0
    measuredContentHeight = pendingContentHeight
    contentFillsViewport = pendingFillsViewport
    updateFrameHeight()
  })
}

function onMeasurement(event: MessageEvent) {
  const frame = frameRef.value
  const data = event.data as {
    type?: unknown
    token?: unknown
    height?: unknown
    fillsViewport?: unknown
  } | null
  if (
    !frame
    || event.source !== frame.contentWindow
    || !data
    || data.type !== ARTIFACT_SIZE_MESSAGE
    || data.token !== sandboxNonce.value
    || typeof data.height !== 'number'
    || !Number.isFinite(data.height)
    || data.height < 0
    || typeof data.fillsViewport !== 'boolean'
  ) return
  scheduleFrameHeight(data.height, data.fillsViewport)
}

watch(stageRef, stage => {
  stageResizeObserver?.disconnect()
  stageResizeObserver = null
  if (!stage) return
  stageResizeObserver = new ResizeObserver(updateFrameHeight)
  stageResizeObserver.observe(stage)
  void nextTick(updateFrameHeight)
})

watch(() => props.autoOpen, maybeAutoOpen)

onMounted(() => {
  window.addEventListener('message', onMeasurement)
  const root = cardRef.value?.closest<HTMLElement>('.session-viewport-scroll') ?? null
  if (typeof IntersectionObserver === 'undefined') {
    maybeAutoOpen()
    return
  }
  visibilityObserver = new IntersectionObserver(([entry]) => {
    inPreviewRange = entry?.isIntersecting ?? false
    if (inPreviewRange) {
      clearDisposeTimer()
      maybeAutoOpen()
    } else {
      scheduleDispose()
    }
  }, { root, rootMargin: '200px 0px' })
  if (cardRef.value) visibilityObserver.observe(cardRef.value)
})

onUnmounted(() => {
  loadRevision++
  clearDisposeTimer()
  visibilityObserver?.disconnect()
  stageResizeObserver?.disconnect()
  if (heightFrame) window.cancelAnimationFrame(heightFrame)
  window.removeEventListener('message', onMeasurement)
})
</script>

<template>
  <article ref="cardRef" class="artifact-card shadow-paper">
    <header class="artifact-card-header">
      <span class="i-carbon-application-web h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
      <div class="min-w-0 flex-1">
        <div class="flex min-w-0 items-center gap-1.5">
          <span class="truncate text-xs font-semibold text-foreground">{{ fileName }}</span>
          <span class="artifact-type-badge">{{ kindLabel }}</span>
          <span v-if="artifact" class="shrink-0 text-[10px] text-muted-foreground">{{ formatBytes(artifact.sizeBytes) }}</span>
        </div>
        <div class="truncate font-mono text-[10px] text-muted-foreground" :title="candidate.path">{{ candidate.path }}</div>
      </div>
      <button
        v-if="artifact"
        type="button"
        class="artifact-icon-button"
        :aria-label="$t('artifactPreview.reload')"
        :title="$t('artifactPreview.reload')"
        :disabled="loading"
        @click="reloadPreview"
      >
        <span class="i-carbon-renew h-3.5 w-3.5" :class="loading && 'animate-spin'" />
      </button>
      <button
        type="button"
        class="artifact-action-button"
        :aria-expanded="expanded"
        :disabled="loading"
        @click="togglePreview"
      >
        <span :class="loading ? 'i-carbon-renew animate-spin' : expanded ? 'i-carbon-chevron-up' : 'i-carbon-view'" class="h-3.5 w-3.5" />
        {{ loading ? $t('common.loading') : expanded ? $t('artifactPreview.collapse') : $t('artifactPreview.preview') }}
      </button>
    </header>

    <div v-if="error" role="alert" class="border-t border-border px-3 py-2 text-xs text-destructive">
      {{ error }}
    </div>

    <div v-if="expanded && artifact" ref="stageRef" class="artifact-stage">
      <iframe
        v-if="artifact.kind === 'html'"
        ref="frameRef"
        :srcdoc="sandboxedHtml"
        sandbox="allow-scripts"
        referrerpolicy="no-referrer"
        loading="lazy"
        class="artifact-frame"
        :style="{ height: `${frameHeight}px` }"
        :title="$t('artifactPreview.frameTitle', { name: fileName })"
      />
      <img
        v-else-if="imageSource"
        :src="imageSource"
        :alt="fileName"
        loading="lazy"
        decoding="async"
        class="artifact-image"
      />
    </div>

    <p v-if="expanded && artifact?.kind === 'html'" class="artifact-sandbox-note">
      <span class="i-carbon-locked h-3 w-3" aria-hidden="true" />
      {{ $t('artifactPreview.sandboxNote') }}
    </p>
  </article>
</template>

<style scoped>
.artifact-card {
  min-width: 0;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
}
.artifact-card-header {
  display: flex;
  min-height: 46px;
  align-items: center;
  gap: 8px;
  padding: 7px 8px 7px 10px;
}
.artifact-type-badge {
  flex: none;
  border: 1px solid var(--border);
  border-radius: 3px;
  background: var(--muted);
  padding: 1px 4px;
  color: var(--muted-foreground);
  font-size: 9px;
  font-weight: 700;
  line-height: 1.4;
}
.artifact-action-button,
.artifact-icon-button {
  display: inline-flex;
  flex: none;
  align-items: center;
  justify-content: center;
  gap: 4px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  color: var(--foreground);
  transition: background-color 120ms ease, box-shadow 120ms ease;
}
.artifact-action-button {
  min-height: 28px;
  padding: 4px 8px;
  background: var(--secondary);
  font-size: 11px;
  font-weight: 600;
}
.artifact-icon-button {
  width: 28px;
  height: 28px;
  background: transparent;
}
.artifact-action-button:hover:not(:disabled),
.artifact-icon-button:hover:not(:disabled) { background: var(--muted); }
.artifact-action-button:focus-visible,
.artifact-icon-button:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 1px;
}
.artifact-action-button:disabled,
.artifact-icon-button:disabled { cursor: wait; opacity: 0.55; }
.artifact-stage {
  display: flex;
  min-height: 120px;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  border-top: 1px solid var(--border);
  background: var(--background);
}
.artifact-frame {
  display: block;
  width: 100%;
  max-height: 60vh;
  border: 0;
  background: var(--card);
}
.artifact-image {
  display: block;
  max-width: 100%;
  max-height: 420px;
  object-fit: contain;
}
.artifact-sandbox-note {
  display: flex;
  align-items: center;
  gap: 4px;
  border-top: 1px solid var(--border);
  padding: 5px 9px;
  color: var(--muted-foreground);
  font-size: 10px;
}
@media (prefers-reduced-motion: reduce) {
  .artifact-action-button,
  .artifact-icon-button { transition: none; }
}
</style>

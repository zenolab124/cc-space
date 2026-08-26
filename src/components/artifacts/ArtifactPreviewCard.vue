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
  ARTIFACT_RUNTIME_BLOCKED_MESSAGE,
  ARTIFACT_SIZE_MESSAGE,
  ARTIFACT_WHEEL_BOUNDARY_MESSAGE,
  MIN_ARTIFACT_FRAME_HEIGHT,
  clampArtifactFrameHeight,
  prepareSandboxedHtml,
} from '@/features/artifact-preview/sandboxHtml'
import {
  handoffManagedFrameWheel,
  registerManagedScrollFrame,
  type ScrollAxis,
} from '@/lib/scrollGestureCoordinator'
import { showImageContextMenu } from '@/composables/useImageActions'

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
const scrollShieldRef = ref<HTMLElement | null>(null)
const imageOpenButtonRef = ref<HTMLButtonElement | null>(null)
const lightboxCloseRef = ref<HTMLButtonElement | null>(null)
const artifact = ref<LoadedArtifact | null>(null)
const loading = ref(false)
const expanded = ref(false)
const error = ref<string | null>(null)
const imageLightboxOpen = ref(false)
const artifactScriptsAllowed = ref(false)
const sandboxScriptNonce = ref('')
const sandboxMessageToken = ref('')
const frameHeight = ref(MIN_ARTIFACT_FRAME_HEIGHT)
let visibilityObserver: IntersectionObserver | null = null
let stageResizeObserver: ResizeObserver | null = null
let disposeTimer: number | null = null
let unregisterScrollFrame: (() => void) | null = null
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
  if (!source || !sandboxScriptNonce.value || !sandboxMessageToken.value) return ''
  return prepareSandboxedHtml(source, {
    scriptNonce: sandboxScriptNonce.value,
    messageToken: sandboxMessageToken.value,
    allowArtifactScripts: artifactScriptsAllowed.value,
  })
})

function rotateSandboxIdentity() {
  sandboxScriptNonce.value = crypto.randomUUID()
  sandboxMessageToken.value = crypto.randomUUID()
}

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
    artifactScriptsAllowed.value = false
    if (loaded.kind === 'html') rotateSandboxIdentity()
    else {
      sandboxScriptNonce.value = ''
      sandboxMessageToken.value = ''
    }
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
  closeImageLightbox(false)
  loadRevision++
  artifact.value = null
  artifactScriptsAllowed.value = false
  sandboxScriptNonce.value = ''
  sandboxMessageToken.value = ''
  void loadPreview()
}

function toggleArtifactScripts() {
  artifactScriptsAllowed.value = !artifactScriptsAllowed.value
  error.value = null
  rotateSandboxIdentity()
  resetFrameMeasurement()
}

function clearDisposeTimer() {
  if (disposeTimer === null) return
  window.clearTimeout(disposeTimer)
  disposeTimer = null
}

function disposePreview() {
  closeImageLightbox(false)
  clearDisposeTimer()
  loadRevision++
  loading.value = false
  expanded.value = false
  artifact.value = null
  artifactScriptsAllowed.value = false
  sandboxScriptNonce.value = ''
  sandboxMessageToken.value = ''
  resetFrameMeasurement()
}

function openImageLightbox() {
  if (!imageSource.value) return
  imageLightboxOpen.value = true
  void nextTick(() => lightboxCloseRef.value?.focus())
}

function closeImageLightbox(restoreFocus = true) {
  if (!imageLightboxOpen.value) return
  imageLightboxOpen.value = false
  if (restoreFocus) void nextTick(() => imageOpenButtonRef.value?.focus())
}

function onLightboxKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') closeImageLightbox()
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
  pendingFillsViewport = pendingFillsViewport && fillsViewport
  if (heightFrame) return
  heightFrame = window.requestAnimationFrame(() => {
    heightFrame = 0
    measuredContentHeight = pendingContentHeight
    // 一旦确认 body 只是被 vh/svh 的最小高度撑开，本次预览保持固有内容高度，
    // 避免收缩后内容与 iframe 等高又被误判为需要最大高度。
    contentFillsViewport = contentFillsViewport && pendingFillsViewport
    pendingFillsViewport = true
    updateFrameHeight()
  })
}

function onSandboxMessage(event: MessageEvent) {
  const frame = frameRef.value
  const data = event.data as {
    type?: unknown
    token?: unknown
    height?: unknown
    fillsViewport?: unknown
    axis?: unknown
    deltaX?: unknown
    deltaY?: unknown
    deltaMode?: unknown
  } | null
  if (
    !frame
    || event.source !== frame.contentWindow
    || !data
    || data.token !== sandboxMessageToken.value
  ) return

  if (data.type === ARTIFACT_RUNTIME_BLOCKED_MESSAGE) {
    artifactScriptsAllowed.value = false
    error.value = t('artifactPreview.runtimeBlocked')
    rotateSandboxIdentity()
    return
  }

  if (data.type === ARTIFACT_WHEEL_BOUNDARY_MESSAGE) {
    if (
      (data.axis !== 'x' && data.axis !== 'y')
      || typeof data.deltaX !== 'number'
      || !Number.isFinite(data.deltaX)
      || typeof data.deltaY !== 'number'
      || !Number.isFinite(data.deltaY)
      || typeof data.deltaMode !== 'number'
      || ![0, 1, 2].includes(data.deltaMode)
    ) return
    handoffManagedFrameWheel(frame, {
      axis: data.axis as ScrollAxis,
      deltaX: data.deltaX,
      deltaY: data.deltaY,
      deltaMode: data.deltaMode,
    })
    return
  }

  if (
    data.type !== ARTIFACT_SIZE_MESSAGE
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

watch([frameRef, scrollShieldRef], ([frame, shield]) => {
  unregisterScrollFrame?.()
  unregisterScrollFrame = frame && shield ? registerManagedScrollFrame(frame, shield) : null
}, { flush: 'post' })

watch(() => props.autoOpen, maybeAutoOpen)

watch(imageLightboxOpen, open => {
  if (open) window.addEventListener('keydown', onLightboxKeydown)
  else window.removeEventListener('keydown', onLightboxKeydown)
})

onMounted(() => {
  window.addEventListener('message', onSandboxMessage)
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
  unregisterScrollFrame?.()
  unregisterScrollFrame = null
  if (heightFrame) window.cancelAnimationFrame(heightFrame)
  window.removeEventListener('keydown', onLightboxKeydown)
  window.removeEventListener('message', onSandboxMessage)
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

    <div v-if="expanded && artifact?.kind === 'html'" class="artifact-runtime-bar">
      <p class="artifact-runtime-status" aria-live="polite">
        <span
          :class="artifactScriptsAllowed ? 'i-carbon-unlocked' : 'i-carbon-locked'"
          class="h-3.5 w-3.5 shrink-0"
          aria-hidden="true"
        />
        {{ artifactScriptsAllowed ? $t('artifactPreview.interactiveNote') : $t('artifactPreview.sandboxNote') }}
      </p>
      <button
        type="button"
        class="artifact-script-button"
        :class="artifactScriptsAllowed && 'is-active'"
        :aria-pressed="artifactScriptsAllowed"
        @click="toggleArtifactScripts"
      >
        <span :class="artifactScriptsAllowed ? 'i-carbon-stop-filled' : 'i-carbon-play-filled-alt'" class="h-3.5 w-3.5" aria-hidden="true" />
        {{ artifactScriptsAllowed ? $t('artifactPreview.disableScripts') : $t('artifactPreview.allowScripts') }}
      </button>
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
      <div
        v-if="artifact.kind === 'html'"
        ref="scrollShieldRef"
        class="artifact-scroll-shield"
        aria-hidden="true"
      />
      <button
        v-else-if="imageSource"
        ref="imageOpenButtonRef"
        type="button"
        class="artifact-image-open"
        :aria-label="`${t('artifactPreview.preview')}: ${fileName}`"
        @click="openImageLightbox"
      >
        <img
          :src="imageSource"
          :alt="fileName"
          :data-image-path="candidate.path"
          loading="lazy"
          decoding="async"
          class="artifact-image"
        />
      </button>
    </div>

    <Teleport to="body">
      <div
        v-if="imageLightboxOpen && imageSource"
        class="artifact-image-lightbox"
        role="dialog"
        aria-modal="true"
        :aria-label="fileName"
        @click.self="closeImageLightbox()"
        @wheel.prevent.stop
      >
        <img
          :src="imageSource"
          :alt="fileName"
          class="artifact-image-lightbox-content"
          decoding="async"
          @contextmenu="showImageContextMenu($event, { src: imageSource, path: candidate.path, fileRoot: root })"
        />
        <button
          ref="lightboxCloseRef"
          type="button"
          class="artifact-image-lightbox-close"
          :aria-label="t('common.close')"
          :title="t('common.close')"
          @click="closeImageLightbox()"
        >
          <span class="i-carbon-close h-5 w-5" aria-hidden="true" />
        </button>
      </div>
    </Teleport>
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
.artifact-runtime-bar {
  display: flex;
  min-height: 38px;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  border-top: 1px solid var(--border);
  padding: 5px 8px 5px 10px;
  background: var(--muted);
}
.artifact-runtime-status {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 5px;
  margin: 0;
  color: var(--muted-foreground);
  font-size: 11px;
  line-height: 1.35;
}
.artifact-script-button {
  display: inline-flex;
  min-height: 28px;
  flex: none;
  align-items: center;
  justify-content: center;
  gap: 4px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 4px 8px;
  color: var(--foreground);
  background: var(--card);
  font-size: 11px;
  font-weight: 600;
  transition: background-color 120ms ease, border-color 120ms ease, color 120ms ease;
}
.artifact-script-button:hover { background: var(--secondary); }
.artifact-script-button.is-active {
  border-color: var(--primary);
  color: var(--primary-foreground);
  background: var(--primary);
}
.artifact-script-button.is-active:hover { opacity: 0.9; }
.artifact-script-button:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 1px;
}
.artifact-stage {
  position: relative;
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
.artifact-scroll-shield {
  position: absolute;
  z-index: 1;
  inset: 0;
  pointer-events: none;
}
.artifact-scroll-shield[data-monet-scroll-shield-active] {
  pointer-events: auto;
}
.artifact-image {
  display: block;
  max-width: 100%;
  max-height: 420px;
  object-fit: contain;
}
.artifact-image-open {
  display: flex;
  max-width: 100%;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: zoom-in;
}
.artifact-image-open:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: -2px;
}
.artifact-image-lightbox {
  position: fixed;
  z-index: 9999;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 5vh 5vw;
  background: rgb(0 0 0 / 0.76);
  backdrop-filter: blur(4px);
}
.artifact-image-lightbox-content {
  display: block;
  max-width: 90vw;
  max-height: 90vh;
  object-fit: contain;
  border-radius: 6px;
}
.artifact-image-lightbox-close {
  position: absolute;
  top: 18px;
  right: 18px;
  display: inline-flex;
  width: 34px;
  height: 34px;
  align-items: center;
  justify-content: center;
  border: 1px solid rgb(255 255 255 / 0.24);
  border-radius: 50%;
  color: white;
  background: rgb(0 0 0 / 0.48);
}
.artifact-image-lightbox-close:hover { background: rgb(0 0 0 / 0.7); }
.artifact-image-lightbox-close:focus-visible {
  outline: 2px solid white;
  outline-offset: 2px;
}
@media (prefers-reduced-motion: reduce) {
  .artifact-action-button,
  .artifact-icon-button,
  .artifact-script-button { transition: none; }
}
</style>

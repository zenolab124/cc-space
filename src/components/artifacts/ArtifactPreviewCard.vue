<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { formatBytes } from '@/types'
import {
  artifactFileName,
  type ArtifactCandidate,
  type ArtifactKind,
} from '@/features/artifact-preview/detectArtifacts'

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
}>()

const { t } = useI18n()
const cardRef = ref<HTMLElement | null>(null)
const artifact = ref<LoadedArtifact | null>(null)
const loading = ref(false)
const expanded = ref(false)
const error = ref<string | null>(null)
let observer: IntersectionObserver | null = null

const fileName = computed(() => artifact.value?.fileName || artifactFileName(props.candidate.path))
const isHtml = computed(() => props.candidate.kind === 'html')
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

function prepareSandboxedHtml(source: string): string {
  const document = new DOMParser().parseFromString(source, 'text/html')
  document.querySelectorAll('meta[http-equiv]').forEach(element => {
    if (element.getAttribute('http-equiv')?.toLowerCase() === 'refresh') element.remove()
  })
  document.querySelectorAll('a, area').forEach(element => {
    element.removeAttribute('href')
    element.removeAttribute('xlink:href')
  })
  document.querySelectorAll('base').forEach(element => element.remove())

  const meta = document.createElement('meta')
  meta.httpEquiv = 'Content-Security-Policy'
  meta.content = [
    "default-src 'none'",
    "img-src data: blob:",
    "media-src data: blob:",
    "font-src data:",
    "style-src 'unsafe-inline'",
    "script-src 'none'",
    "connect-src 'none'",
    "frame-src 'none'",
    "child-src 'none'",
    "worker-src 'none'",
    "object-src 'none'",
    "form-action 'none'",
    "base-uri 'none'",
  ].join('; ')
  document.head.prepend(meta)
  return `<!doctype html>\n${document.documentElement.outerHTML}`
}

const sandboxedHtml = computed(() => {
  const source = artifact.value?.text
  if (!source) return ''
  return prepareSandboxedHtml(source)
})

async function loadPreview() {
  if (loading.value) return
  loading.value = true
  error.value = null
  try {
    artifact.value = await invoke<LoadedArtifact>('read_artifact_preview', {
      root: props.root,
      path: props.candidate.path,
    })
    expanded.value = true
  } catch (cause) {
    error.value = String(cause)
  } finally {
    loading.value = false
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
  artifact.value = null
  void loadPreview()
}

onMounted(() => {
  if (isHtml.value) return
  if (typeof IntersectionObserver === 'undefined') {
    void loadPreview()
    return
  }
  observer = new IntersectionObserver(entries => {
    if (!entries.some(entry => entry.isIntersecting)) return
    observer?.disconnect()
    observer = null
    void loadPreview()
  }, { rootMargin: '280px' })
  if (cardRef.value) observer.observe(cardRef.value)
})

onUnmounted(() => observer?.disconnect())
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

    <div v-if="expanded && artifact" class="artifact-stage">
      <iframe
        v-if="artifact.kind === 'html'"
        :srcdoc="sandboxedHtml"
        sandbox=""
        referrerpolicy="no-referrer"
        loading="lazy"
        class="artifact-frame"
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
  max-height: 440px;
  align-items: center;
  justify-content: center;
  overflow: auto;
  border-top: 1px solid var(--border);
  background: var(--background);
}
.artifact-frame {
  display: block;
  width: 100%;
  height: 380px;
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

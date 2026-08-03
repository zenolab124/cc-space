<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'

const IMAGE_EXTS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico'])

const props = defineProps<{
  input: Record<string, unknown>
  toolUseId: string
  name: string
}>()

const { t } = useI18n()

interface FileEntry {
  path: string
  ext: string
  fileName: string
  isImage: boolean
  dataUrl: string | null
  loading: boolean
  error: string | null
}

const files = ref<FileEntry[]>([])
const lightboxSrc = ref<string | null>(null)

const caption = props.input.caption as string | undefined
const rawFiles = (props.input.files ?? []) as string[]

function getExt(path: string): string {
  const dot = path.lastIndexOf('.')
  return dot > -1 ? path.slice(dot + 1).toLowerCase() : ''
}

function getFileName(path: string): string {
  const sep = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return sep > -1 ? path.slice(sep + 1) : path
}

onMounted(() => {
  files.value = rawFiles.map(p => {
    const ext = getExt(p)
    return {
      path: p,
      ext,
      fileName: getFileName(p),
      isImage: IMAGE_EXTS.has(ext),
      dataUrl: null,
      loading: IMAGE_EXTS.has(ext),
      error: null,
    }
  })
  files.value.forEach((f, i) => {
    if (f.isImage) loadImage(i)
  })
})

async function loadImage(index: number) {
  const f = files.value[index]
  try {
    const result = await invoke<{ data: string; mime_type: string }>('read_local_image', { path: f.path })
    f.dataUrl = `data:${result.mime_type};base64,${result.data}`
  } catch (e) {
    f.error = String(e)
  } finally {
    f.loading = false
  }
}

function openFile(path: string) {
  invoke('open_in_default_app', { path })
}

function revealInFinder(path: string) {
  // 文件管理器中显示并高亮该文件(Rust 侧平台分支;Linux 退化开所在目录)
  invoke('reveal_in_finder', { path })
}
</script>

<template>
  <div class="mt-2 rounded-md bg-background border border-border px-3 py-2 text-xs space-y-2">
    <div class="flex items-start gap-1.5 min-w-0">
      <span class="i-carbon-image w-3.5 h-3.5 mt-0.5 shrink-0 text-muted-foreground" />
      <span class="text-foreground font-medium shrink-0 whitespace-nowrap">{{ t('block.toolSendFile.title') }}</span>
      <span v-if="caption" class="min-w-0 flex-1 ml-1 text-muted-foreground leading-relaxed break-words">— {{ caption }}</span>
    </div>

    <div v-for="(f, i) in files" :key="i" class="space-y-1">
      <!-- 图片 -->
      <template v-if="f.isImage">
        <div v-if="f.loading" class="h-32 rounded bg-muted flex items-center justify-center">
          <span class="i-carbon-loading animate-spin w-4 h-4 text-muted-foreground" />
        </div>
        <div v-else-if="f.dataUrl" class="relative group">
          <img
            :src="f.dataUrl"
            :alt="f.fileName"
            class="max-w-full max-h-80 rounded border border-border cursor-zoom-in object-contain"
            @click="lightboxSrc = f.dataUrl"
          >
          <div class="absolute top-1 right-1 opacity-0 group-hover:opacity-100 transition-opacity flex gap-1">
            <button
              class="p-1 rounded bg-background/80 border border-border hover:bg-muted"
              :title="t('block.toolSendFile.openFile')"
              @click.stop="openFile(f.path)"
            >
              <span class="i-carbon-launch w-3 h-3" />
            </button>
            <button
              class="p-1 rounded bg-background/80 border border-border hover:bg-muted"
              :title="t('block.toolSendFile.revealInFinder')"
              @click.stop="revealInFinder(f.path)"
            >
              <span class="i-carbon-folder w-3 h-3" />
            </button>
          </div>
        </div>
        <div v-else-if="f.error" class="rounded bg-muted px-2 py-1 text-muted-foreground">
          <span class="i-carbon-warning w-3 h-3 inline-block mr-1" />
          {{ f.fileName }}: {{ f.error }}
        </div>
      </template>

      <!-- 非图片文件 -->
      <div v-else class="flex items-center gap-2 rounded bg-muted px-2 py-1.5 group">
        <span class="i-carbon-document w-4 h-4 shrink-0 text-muted-foreground" />
        <span class="font-mono text-muted-foreground truncate flex-1" :title="f.path">{{ f.fileName }}</span>
        <button
          class="p-1 rounded hover:bg-background opacity-0 group-hover:opacity-100 transition-opacity"
          :title="t('block.toolSendFile.openFile')"
          @click="openFile(f.path)"
        >
          <span class="i-carbon-launch w-3 h-3" />
        </button>
        <button
          class="p-1 rounded hover:bg-background opacity-0 group-hover:opacity-100 transition-opacity"
          :title="t('block.toolSendFile.revealInFinder')"
          @click="revealInFinder(f.path)"
        >
          <span class="i-carbon-folder w-3 h-3" />
        </button>
      </div>
    </div>

    <!-- Lightbox -->
    <Teleport to="body">
      <div
        v-if="lightboxSrc"
        class="fixed inset-0 z-999 flex items-center justify-center bg-black/80 cursor-zoom-out"
        @click="lightboxSrc = null"
      >
        <img :src="lightboxSrc" class="max-w-[90vw] max-h-[90vh] object-contain rounded-lg shadow-2xl">
      </div>
    </Teleport>
  </div>
</template>

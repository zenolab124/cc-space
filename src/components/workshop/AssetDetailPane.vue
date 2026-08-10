<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { renderMarkdownCached } from '@/composables/useMarkdown'
import { showSystemOpenMenu } from '@/composables/useFileOpener'
import type { AssetDetail } from '@/types'

/**
 * 资产详情面板（v2.9.0 FR-001/FR-002）：
 * 接收选中资产 path，调用 get_asset_detail 加载 frontmatter + body 并渲染。
 * 头部：名称 + 版本徽章 + 来源徽章 + path 等宽小字 + 「打开文件」按钮。
 * frontmatter 卡 + markdown 正文。加载态/错误态/文件不存在态。
 */

const props = defineProps<{
  path: string | null
  name?: string
  version?: string | null
  source?: string
}>()

const emit = defineEmits<{
  (e: 'refresh'): void
}>()

const { t } = useI18n()

// 状态
const detail = ref<AssetDetail | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const openFailed = ref(false)
let openFailTimer: ReturnType<typeof setTimeout> | undefined

// path 变化时重新加载
watch(() => props.path, async (newPath) => {
  detail.value = null
  error.value = null
  if (!newPath) return
  await loadDetail(newPath)
}, { immediate: true })

async function loadDetail(path: string) {
  loading.value = true
  error.value = null
  try {
    detail.value = await invoke<AssetDetail>('get_asset_detail', { path })
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

function retry() {
  if (props.path) loadDetail(props.path)
}

async function openFile(systemDefault = false) {
  if (!props.path) return
  try {
    await invoke('open_asset_file', { path: props.path, systemDefault })
  } catch (_) {
    openFailed.value = true
    clearTimeout(openFailTimer)
    openFailTimer = setTimeout(() => { openFailed.value = false }, 3000)
  }
}

function showFileMenu(event: MouseEvent) {
  if (!props.path) return
  return showSystemOpenMenu(event, () => openFile(true), props.path)
}

/** frontmatter key-value 对列表 */
const frontmatterEntries = computed(() => {
  const fm = detail.value?.frontmatter
  if (!fm) return []
  return Object.entries(fm).map(([key, value]) => ({
    key,
    value: typeof value === 'object' ? JSON.stringify(value) : String(value ?? ''),
  }))
})

/**
 * markdown 正文渲染：复用现有 renderMarkdownCached 管线。
 * 相对链接与图片渲染为纯文本样式（不加载不跳转），通过 CSS 处理。
 */
const renderedBody = computed(() => {
  if (!detail.value?.body) return ''
  return renderMarkdownCached(detail.value.body)
})

/** 是否为「文件不存在」错误 */
const isFileNotExist = computed(() => {
  return error.value?.includes('not found') || error.value?.includes('不存在') || error.value?.includes('No such file')
})
</script>

<template>
  <!-- 空态：未选中 -->
  <div v-if="!path" class="detail-empty">
    <span class="text-xs text-muted-foreground">{{ t('workshop.selectToView') }}</span>
  </div>

  <!-- 加载态 -->
  <div v-else-if="loading" class="detail-empty">
    <span class="text-xs text-muted-foreground">{{ t('workshop.loading') }}</span>
  </div>

  <!-- 错误态 -->
  <div v-else-if="error" class="detail-error">
    <p class="text-xs text-destructive">
      {{ isFileNotExist ? t('workshop.fileNotExist') : t('workshop.loadFailed') }}
    </p>
    <div class="detail-error-actions">
      <button v-if="isFileNotExist" class="ws-btn" @click="emit('refresh')">
        {{ t('workshop.refreshList') }}
      </button>
      <button v-else class="ws-btn" @click="retry">{{ t('workshop.retry') }}</button>
    </div>
  </div>

  <!-- 正常态 -->
  <div v-else-if="detail" class="detail-content">
    <!-- 头部 -->
    <div class="detail-head">
      <h1 class="detail-title">
        <span>{{ name ?? '' }}</span>
        <span v-if="version" class="detail-version">v{{ version }}</span>
        <span v-if="source" class="detail-source-badge">{{ source }}</span>
      </h1>
      <div class="detail-path">{{ path }}</div>
      <div class="detail-actions">
        <button class="ws-btn" @click="openFile()" @contextmenu="showFileMenu">
          <span class="i-carbon-launch w-3 h-3" />
          {{ t('workshop.openFile') }}
        </button>
        <span v-if="openFailed" class="text-xs text-destructive">{{ t('workshop.openFileFailed') }}</span>
      </div>
    </div>

    <!-- frontmatter 卡 -->
    <div v-if="detail.frontmatter" class="fm-card">
      <div class="fm-title">{{ t('workshop.frontmatter') }}</div>
      <div v-for="entry in frontmatterEntries" :key="entry.key" class="fm-row">
        <span class="fm-key">{{ entry.key }}</span>
        <span class="fm-val">{{ entry.value }}</span>
      </div>
    </div>
    <!-- frontmatter 解析失败提示 -->
    <div v-else class="fm-warn">
      <span class="i-carbon-warning w-3 h-3" />
      {{ t('workshop.frontmatterParseFailed') }}
    </div>

    <!-- 正文 markdown 渲染 -->
    <div v-if="detail.body" class="md-body asset-md-body" v-html="renderedBody" />

    <!-- 截断提示 -->
    <div v-if="detail.truncated" class="truncated-hint">
      {{ t('workshop.truncatedHint') }}
    </div>
  </div>
</template>

<style scoped>
.detail-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  min-height: 200px;
}
.detail-error {
  padding: 32px 20px;
  text-align: center;
}
.detail-error-actions {
  margin-top: 12px;
}
.detail-content {
  padding: 20px 26px;
}
.detail-head {
  margin-bottom: 16px;
}
.detail-title {
  font-size: 16px;
  font-weight: 700;
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.detail-version {
  font-size: 10px;
  font-family: var(--font-mono);
  font-weight: 400;
  color: var(--muted-foreground);
  border: 1px solid var(--border);
  border-radius: 3px;
  padding: 0 5px;
}
.detail-source-badge {
  font-size: 10px;
  border: 1px solid var(--border);
  border-radius: 3px;
  padding: 0 5px;
  color: var(--muted-foreground);
}
.detail-path {
  font-size: 10.5px;
  font-family: var(--font-mono);
  color: var(--muted-foreground);
  margin-top: 4px;
  word-break: break-all;
}
.detail-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 8px;
}

/* frontmatter 卡 */
.fm-card {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  box-shadow: var(--shadow-paper);
  overflow: hidden;
  margin-bottom: 16px;
}
.fm-title {
  font-size: 11px;
  font-weight: 600;
  padding: 8px 12px;
  background: var(--secondary);
  border-bottom: 1px solid var(--border);
  color: var(--muted-foreground);
}
.fm-row {
  display: flex;
  padding: 6px 12px;
  border-bottom: 1px solid var(--border);
  font-size: 11.5px;
  gap: 8px;
}
.fm-row:last-child {
  border-bottom: none;
}
.fm-key {
  width: 110px;
  flex-shrink: 0;
  color: var(--muted-foreground);
  font-family: var(--font-mono);
  font-size: 10.5px;
}
.fm-val {
  flex: 1;
  min-width: 0;
  word-break: break-word;
}

/* frontmatter 解析失败提示 */
.fm-warn {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--amber, oklch(0.62 0.14 70));
  background: var(--amber-bg, oklch(0.92 0.04 70));
  padding: 6px 10px;
  border-radius: var(--radius);
  margin-bottom: 16px;
}

/* markdown 正文 — 相对链接、图片一律纯文本 */
.asset-md-body :deep(a) {
  color: var(--muted-foreground);
  text-decoration: none;
  pointer-events: none;
  cursor: default;
}
.asset-md-body :deep(img) {
  display: none;
}

/* 截断提示 */
.truncated-hint {
  margin-top: 12px;
  font-size: 10.5px;
  color: var(--amber, oklch(0.62 0.14 70));
  background: var(--amber-bg, oklch(0.92 0.04 70));
  padding: 6px 10px;
  border-radius: var(--radius);
}

/* 按钮样式（与 WorkshopView 一致） */
.ws-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  padding: 3px 12px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--card);
  cursor: pointer;
}
.ws-btn:hover {
  box-shadow: var(--shadow-paper);
}
</style>

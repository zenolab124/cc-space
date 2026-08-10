<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import type { HookEntry, HookEventGroup } from '@/types'
import { useHooks } from '@/composables/useHooks'
import { showSystemOpenMenu } from '@/composables/useFileOpener'

/**
 * Hooks 子页（v2.9.0 FR-005）：
 * 自带两列布局（列表 300px + 详情弹性），按事件分组展示所有已配置 hooks。
 * 数据源在 useHooks module 单例（v-if 挂载下组件态会丢），五色类型徽章，详情显示完整 config。
 */

const { t } = useI18n()

// --- 数据加载（module 单例：一次缓存 + 手动刷新） ---
const { overview, loading, error, loadHooks, ensureLoaded } = useHooks()

onMounted(() => {
  ensureLoaded()
})

// --- 选中态 ---
interface Selection {
  groupIdx: number
  entryIdx: number
}
const selection = ref<Selection | null>(null)

const selectedEntry = computed<HookEntry | null>(() => {
  if (!selection.value || !overview.value) return null
  const group = overview.value.events[selection.value.groupIdx]
  if (!group) return null
  return group.entries[selection.value.entryIdx] ?? null
})

const selectedEvent = computed<string>(() => {
  if (!selection.value || !overview.value) return ''
  return overview.value.events[selection.value.groupIdx]?.event ?? ''
})

// 1-based 序号（组内）
const selectedIndex = computed<number>(() => {
  return selection.value ? selection.value.entryIdx + 1 : 0
})

function selectHook(groupIdx: number, entryIdx: number) {
  selection.value = { groupIdx, entryIdx }
}

function isSelected(groupIdx: number, entryIdx: number): boolean {
  return selection.value?.groupIdx === groupIdx && selection.value?.entryIdx === entryIdx
}

// --- 详情区：打开来源文件 ---
const openFailed = ref(false)
let openFailTimer: ReturnType<typeof setTimeout> | undefined

async function openSourceFile(systemDefault = false) {
  if (!selectedEntry.value) return
  try {
    await invoke('open_asset_file', { path: selectedEntry.value.sourceFile, systemDefault })
    openFailed.value = false
  } catch (_) {
    openFailed.value = true
    clearTimeout(openFailTimer)
    openFailTimer = setTimeout(() => { openFailed.value = false }, 3000)
  }
}

function showSourceFileMenu(event: MouseEvent) {
  const path = selectedEntry.value?.sourceFile
  if (!path) return
  return showSystemOpenMenu(event, () => openSourceFile(true), path)
}

// --- Config 渲染辅助 ---
function formatConfigValue(value: unknown): { isComplex: boolean; text: string } {
  if (value === null || value === undefined) {
    return { isComplex: false, text: String(value) }
  }
  if (typeof value === 'object') {
    return { isComplex: true, text: JSON.stringify(value, null, 2) }
  }
  return { isComplex: false, text: String(value) }
}

// 计数经 useHooks().hooksCount 供 WorkshopView 消费，不再依赖组件实例暴露
</script>

<template>
  <div class="hooks-root">
    <!-- 左列：列表 -->
    <div class="hooks-list-col">
      <!-- 头部 -->
      <div class="hooks-list-head">
        <div class="hooks-head-text">
          <h2 class="hooks-title">{{ t('workshop.hooksTitle') }}</h2>
          <span class="hooks-sub">{{ t('workshop.hooksSubtitle') }}</span>
        </div>
        <button class="ws-btn" :disabled="loading" @click="loadHooks">
          <span class="i-carbon-renew w-3 h-3" :class="{ 'animate-spin': loading }" />
        </button>
      </div>

      <!-- 解析失败警告 -->
      <div v-if="overview && overview.parseFailures.length > 0" class="hooks-parse-warn">
        <span class="i-carbon-warning w-3 h-3" />
        {{ t('workshop.hooksParseWarn', { n: overview.parseFailures.length }) }}
      </div>

      <!-- 加载/错误态 -->
      <div v-if="loading && !overview" class="hooks-status">
        <span class="text-xs text-muted-foreground">{{ t('common.loading') }}</span>
      </div>
      <div v-else-if="error" class="hooks-status">
        <p class="text-xs text-destructive">{{ error }}</p>
        <button class="ws-btn mt-2" @click="loadHooks">{{ t('common.retry') }}</button>
      </div>

      <!-- 空态 -->
      <div v-else-if="overview && overview.events.length === 0" class="hooks-status">
        <p class="text-xs text-muted-foreground">{{ t('workshop.hooksEmpty') }}</p>
        <p class="text-xs text-muted-foreground mt-1 hooks-empty-desc">{{ t('workshop.hooksEmptyDesc') }}</p>
      </div>

      <!-- 事件分组列表 -->
      <div v-else-if="overview" class="hooks-scroll">
        <div v-for="(group, gIdx) in overview.events" :key="group.event" class="hooks-group">
          <!-- 分组头 -->
          <div class="hooks-group-head">
            <span class="hooks-event-name">{{ group.event }}</span>
            <span class="hooks-event-count">({{ group.entries.length }})</span>
          </div>
          <!-- 条目 -->
          <div
            v-for="(entry, eIdx) in group.entries"
            :key="`${gIdx}-${eIdx}`"
            class="hooks-row"
            :class="{ selected: isSelected(gIdx, eIdx) }"
            @click="selectHook(gIdx, eIdx)"
          >
            <span class="hook-type-badge" :class="`hook-type-${entry.hookType}`">
              {{ entry.hookType }}
            </span>
            <span v-if="entry.matcher" class="hook-matcher-badge">{{ entry.matcher }}</span>
            <span class="hook-summary">{{ entry.summary }}</span>
            <span class="hook-source-badge">{{ entry.sourceLayer }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- 右列：详情 -->
    <div class="hooks-detail-col">
      <!-- 空态 -->
      <div v-if="!selectedEntry" class="detail-empty">
        <span class="text-xs text-muted-foreground">{{ t('workshop.selectToView') }}</span>
      </div>

      <!-- 选中态 -->
      <div v-else class="detail-content">
        <!-- 头部 -->
        <div class="detail-head">
          <h3 class="detail-title">{{ selectedEvent }} · Hook #{{ selectedIndex }}</h3>
          <div class="detail-actions">
            <button class="ws-btn" @click="openSourceFile()" @contextmenu="showSourceFileMenu">
              <span class="i-carbon-launch w-3 h-3" />
              {{ t('workshop.hooksOpenSource') }}
            </button>
            <span v-if="openFailed" class="text-xs text-destructive">{{ t('workshop.hooksOpenFailed') }}</span>
          </div>
        </div>

        <!-- 完整配置卡 -->
        <div class="fm-card">
          <div class="fm-title">{{ t('workshop.hooksConfig') }}</div>
          <div v-for="(value, key) in selectedEntry.config" :key="String(key)" class="fm-row">
            <span class="fm-key">{{ String(key) }}</span>
            <span v-if="!formatConfigValue(value).isComplex" class="fm-val">
              {{ formatConfigValue(value).text }}
            </span>
            <pre v-else class="fm-val-code"><code>{{ formatConfigValue(value).text }}</code></pre>
          </div>
          <!-- 配置为空 -->
          <div v-if="Object.keys(selectedEntry.config).length === 0" class="fm-row">
            <span class="fm-val text-muted-foreground">—</span>
          </div>
        </div>

        <!-- 来源信息 -->
        <div class="source-info">
          <span class="source-label">{{ t('workshop.hooksSourceLayer') }}</span>
          <span class="hook-source-badge">{{ selectedEntry.sourceLayer }}</span>
          <span class="source-path">{{ selectedEntry.sourceFile }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 根容器：两列并排 */
.hooks-root {
  display: flex;
  height: 100%;
}

/* 左列 */
.hooks-list-col {
  width: 300px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.hooks-list-head {
  padding: 12px 14px 8px;
  display: flex;
  align-items: center;
  gap: 8px;
  border-bottom: 1px solid var(--border);
}
.hooks-head-text {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
}
.hooks-title {
  font-size: 13px;
  font-weight: 600;
}
.hooks-sub {
  font-size: 10.5px;
  color: var(--muted-foreground);
}

/* 解析失败警告条 */
.hooks-parse-warn {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--amber, oklch(0.62 0.14 70));
  background: var(--amber-bg, oklch(0.92 0.04 70));
  padding: 6px 12px;
  border-bottom: 1px solid var(--border);
}

/* 状态容器 */
.hooks-status {
  padding: 32px 14px;
  text-align: center;
}
.hooks-empty-desc {
  opacity: 0.7;
}

/* 滚动区 */
.hooks-scroll {
  flex: 1;
  overflow-y: auto;
}

/* 事件分组 */
.hooks-group {
  border-bottom: 1px solid var(--border);
}
.hooks-group:last-child {
  border-bottom: none;
}
.hooks-group-head {
  padding: 8px 14px 4px;
  display: flex;
  align-items: center;
  gap: 4px;
  background: var(--secondary);
}
.hooks-event-name {
  font-size: 11px;
  font-weight: 600;
  font-family: var(--font-mono);
}
.hooks-event-count {
  font-size: 10px;
  color: var(--muted-foreground);
}

/* Hook 行 */
.hooks-row {
  padding: 7px 14px;
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  border-bottom: 1px solid var(--border);
}
.hooks-row:last-child {
  border-bottom: none;
}
.hooks-row:hover {
  background: var(--muted);
}
.hooks-row.selected {
  background: var(--secondary);
}

/* 类型徽章：5 色 */
.hook-type-badge {
  font-size: 9px;
  font-weight: 500;
  border-radius: 3px;
  padding: 1px 5px;
  white-space: nowrap;
  flex-shrink: 0;
}
.hook-type-command {
  color: var(--hook-command, oklch(0.50 0.08 145));
  background: var(--hook-command-bg, oklch(0.92 0.03 145));
}
.hook-type-prompt {
  color: var(--hook-prompt, oklch(0.52 0.10 280));
  background: var(--hook-prompt-bg, oklch(0.92 0.03 280));
}
.hook-type-agent {
  color: var(--hook-agent, oklch(0.55 0.10 40));
  background: var(--hook-agent-bg, oklch(0.92 0.04 40));
}
.hook-type-http {
  color: var(--hook-http, oklch(0.48 0.08 220));
  background: var(--hook-http-bg, oklch(0.90 0.03 220));
}
.hook-type-mcp_tool {
  color: var(--hook-mcp, oklch(0.50 0.06 330));
  background: var(--hook-mcp-bg, oklch(0.91 0.03 330));
}

/* Matcher 徽章 */
.hook-matcher-badge {
  font-size: 9px;
  font-weight: 500;
  font-family: var(--font-mono);
  border-radius: 3px;
  padding: 1px 5px;
  white-space: nowrap;
  flex-shrink: 0;
  border: 1px solid var(--border);
  color: var(--muted-foreground);
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Summary 文本 */
.hook-summary {
  flex: 1;
  min-width: 0;
  font-size: 10px;
  font-family: var(--font-mono);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--foreground);
}

/* Source 层徽章（中性边框） */
.hook-source-badge {
  font-size: 9.5px;
  font-weight: 500;
  border-radius: 3px;
  padding: 1px 5px;
  white-space: nowrap;
  flex-shrink: 0;
  border: 1px solid var(--border);
  color: var(--muted-foreground);
}

/* 右列 */
.hooks-detail-col {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}

.detail-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  min-height: 200px;
}

.detail-content {
  padding: 20px 26px;
}

.detail-head {
  margin-bottom: 16px;
}
.detail-title {
  font-size: 15px;
  font-weight: 700;
  font-family: var(--font-mono);
}
.detail-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 8px;
}

/* Config 卡片（复用 AssetDetailPane 的 fm-card 风格） */
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
  align-items: baseline;
}
.fm-row:last-child {
  border-bottom: none;
}
.fm-key {
  width: 120px;
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
.fm-val-code {
  flex: 1;
  min-width: 0;
  margin: 0;
  padding: 4px 8px;
  background: var(--muted);
  border-radius: 3px;
  font-family: var(--font-mono);
  font-size: 10px;
  line-height: 1.5;
  overflow-x: auto;
  white-space: pre;
}
.fm-val-code code {
  font-family: inherit;
  font-size: inherit;
}

/* 来源信息行 */
.source-info {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
}
.source-label {
  color: var(--muted-foreground);
  font-size: 10.5px;
}
.source-path {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--muted-foreground);
  word-break: break-all;
}

/* 按钮（与 WorkshopView 一致） */
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
.ws-btn:hover:not(:disabled) {
  box-shadow: var(--shadow-paper);
}
.ws-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>

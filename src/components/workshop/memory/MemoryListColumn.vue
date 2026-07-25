<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useMemory, MEMORY_STALE_DAYS } from '@/composables/useMemory'
import { relativeTime } from '@/types'
import type { MemoryType, MemoryEntry } from '@/types'

/**
 * 记忆列表列（v2.9.0 FR-007）：
 * 项目下拉 + type 筛选徽签行 + 体检徽章 + 记忆行列表
 */

const { t } = useI18n()
const {
  overview,
  currentProject,
  filteredEntries,
  typeCounts,
  selectedProjectDir,
  filterType,
  selectedFile,
  healthIssueCount,
  loading,
  refresh,
} = useMemory()

const emit = defineEmits<{
  (e: 'toggleHealth'): void
}>()

// 按 lastModified 降序排列的项目列表
const sortedProjects = computed(() => {
  if (!overview.value) return []
  return [...overview.value.projects].sort((a, b) => b.lastModified - a.lastModified)
})

// type 筛选选项
const typeFilters: Array<{ type: MemoryType | null; labelKey: string }> = [
  { type: null, labelKey: 'memory.filterAll' },
  { type: 'project', labelKey: 'memory.typeProject' },
  { type: 'feedback', labelKey: 'memory.typeFeedback' },
  { type: 'user', labelKey: 'memory.typeUser' },
  { type: 'reference', labelKey: 'memory.typeReference' },
]

function selectEntry(entry: MemoryEntry) {
  selectedFile.value = entry.file
}

// 判断是否陈旧
function isStale(entry: MemoryEntry): boolean {
  if (entry.type !== 'project') return false
  const now = Date.now() / 1000
  const days = (now - entry.mtime) / 86400
  return days > MEMORY_STALE_DAYS
}

// type 色值 class
function typeClass(type: MemoryType): string {
  return `mem-type-${type}`
}
</script>

<template>
  <div class="memory-list-col">
    <!-- 列表头 -->
    <div class="list-head">
      <h2 class="list-title">{{ t('memory.title') }}</h2>
      <button
        class="head-refresh"
        :disabled="loading"
        :title="t('common.refresh')"
        @click="refresh"
      >
        <span class="i-carbon-renew w-3 h-3" :class="{ 'animate-spin': loading }" />
      </button>
    </div>

    <!-- 项目下拉 -->
    <select
      v-model="selectedProjectDir"
      class="proj-select"
    >
      <option
        v-for="proj in sortedProjects"
        :key="proj.projectDir"
        :value="proj.projectDir"
      >
        {{ proj.displayName }} · {{ proj.count }} {{ t('memory.entries') }}
      </option>
    </select>

    <!-- type 筛选徽签 -->
    <div class="chip-row">
      <button
        v-for="f in typeFilters"
        :key="f.labelKey"
        class="chip"
        :class="{ active: filterType === f.type }"
        @click="filterType = f.type"
      >
        {{ t(f.labelKey) }} {{ f.type ? typeCounts[f.type] : typeCounts.all }}
      </button>
    </div>

    <!-- 体检徽章 -->
    <div class="health-badge-row">
      <button
        class="badge-health"
        :class="{ 'badge-health-ok': healthIssueCount === 0 }"
        @click="emit('toggleHealth')"
      >
        <span v-if="healthIssueCount > 0" class="i-carbon-warning w-2.5 h-2.5" />
        <span v-else class="i-carbon-checkmark w-2.5 h-2.5" />
        {{ healthIssueCount > 0 ? t('memory.healthIssues', { n: healthIssueCount }) : t('memory.healthOk') }}
      </button>
      <!-- legacyIndex 提示 -->
      <span v-if="currentProject?.legacyIndex" class="legacy-hint">
        {{ t('memory.legacyIndexHint') }}
      </span>
    </div>

    <!-- 体检面板插槽 -->
    <slot name="health" />

    <!-- 记忆行列表 -->
    <div class="entry-list">
      <div
        v-for="entry in filteredEntries"
        :key="entry.file"
        class="fitem"
        :class="{ selected: selectedFile === entry.file }"
        @click="selectEntry(entry)"
      >
        <div class="fi-body">
          <div class="fi-name">
            {{ entry.name }}
            <span v-if="isStale(entry)" class="badge-stale">{{ t('memory.stale') }}</span>
          </div>
          <div class="fi-desc">{{ entry.description }}</div>
        </div>
        <span class="mem-type" :class="typeClass(entry.type)">{{ entry.type }}</span>
        <span class="time-ago">{{ relativeTime(entry.mtime) }}</span>
      </div>

      <!-- 空态 -->
      <div v-if="filteredEntries.length === 0" class="empty-hint">
        {{ t('memory.noEntries') }}
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 列本身不滚：头部固定，只有 .entry-list 与体检面板各自滚动。
   曾经外层 overflow-y:auto + 内层 flex:1（min-height 默认 auto 不收缩）
   两层打架 —— 内层滚动永不生效、整列跟着滚，且外层滚动条使
   .proj-select 的 calc(100%) 算错宽度，与列表项右边距对不齐 */
.memory-list-col {
  width: 300px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.list-head {
  padding: 12px 14px 8px;
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
}
.list-title {
  font-size: 13px;
  font-weight: 600;
}
.head-refresh {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 3px;
  border: none;
  border-radius: var(--radius);
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
  flex-shrink: 0;
}
.head-refresh:hover:not(:disabled) {
  background: var(--muted);
  color: var(--foreground);
}
.head-refresh:disabled {
  opacity: 0.5;
  cursor: default;
}
/* 宽度交给 flex stretch（cross axis）自然填满，不写 calc(100%)——
   后者不扣滚动条宽度，滚动条一出现就与列表项右边距错位 */
.proj-select {
  font-size: 11px;
  padding: 4px 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
  color: var(--foreground);
  margin: 10px 14px 0;
  min-width: 0;
  flex-shrink: 0;
}
.chip-row {
  display: flex;
  gap: 4px;
  padding: 8px 14px;
  flex-wrap: wrap;
  flex-shrink: 0;
}
.chip {
  font-size: 10px;
  padding: 2px 8px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--muted-foreground);
  cursor: pointer;
}
.chip:hover {
  background: var(--muted);
}
.chip.active {
  background: var(--primary);
  color: var(--primary-foreground);
  border-color: var(--primary);
}
.health-badge-row {
  padding: 4px 14px 6px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
/* 体检面板自身无高度约束，问题多时会把列表挤没 —— 限高自滚，
   保证记忆列表始终占住至少一半 */
:slotted(.health-panel) {
  flex-shrink: 0;
  max-height: 45%;
  overflow-y: auto;
}
.badge-health {
  font-size: 9.5px;
  font-weight: 500;
  border-radius: 3px;
  padding: 2px 6px;
  white-space: nowrap;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  background: var(--amber-bg, oklch(0.92 0.04 70));
  color: var(--amber, oklch(0.62 0.14 70));
  border: none;
}
.badge-health-ok {
  background: var(--mem-project-bg, oklch(0.92 0.03 145));
  color: var(--mem-project, var(--primary));
}
.legacy-hint {
  font-size: 9.5px;
  color: var(--amber, oklch(0.62 0.14 70));
}
.entry-list {
  flex: 1;
  min-height: 0; /* flex item 默认 min-height:auto 不收缩，滚动区必须显式归零 */
  overflow-y: auto;
}
.fitem {
  background: var(--card);
  border-bottom: 1px solid var(--border);
  padding: 10px 14px;
  display: flex;
  align-items: center;
  gap: 10px;
  cursor: pointer;
}
.fitem:hover {
  background: var(--muted);
}
.fitem.selected {
  background: var(--secondary);
}
.fi-body {
  flex: 1;
  min-width: 0;
}
.fi-name {
  font-size: 12px;
  font-weight: 600;
  display: flex;
  align-items: baseline;
  gap: 5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.fi-desc {
  font-size: 11px;
  color: var(--muted-foreground);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 1px;
}
.mem-type {
  font-size: 9px;
  font-weight: 600;
  border-radius: 3px;
  padding: 1px 5px;
  white-space: nowrap;
  flex-shrink: 0;
}
.mem-type-project {
  background: var(--mem-project-bg, oklch(0.92 0.03 145));
  color: var(--mem-project, var(--primary));
}
.mem-type-feedback {
  background: var(--mem-feedback-bg, oklch(0.92 0.03 280));
  color: var(--mem-feedback, oklch(0.52 0.10 280));
}
.mem-type-user {
  background: var(--mem-user-bg, oklch(0.92 0.04 40));
  color: var(--mem-user, oklch(0.55 0.10 40));
}
.mem-type-reference {
  background: var(--mem-reference-bg, oklch(0.90 0.03 220));
  color: var(--mem-reference, oklch(0.50 0.08 220));
}
.mem-type-unknown {
  background: var(--muted);
  color: var(--muted-foreground);
}
.badge-stale {
  font-size: 8.5px;
  font-weight: 500;
  border-radius: 3px;
  padding: 0 4px;
  background: var(--amber-bg, oklch(0.92 0.04 70));
  color: var(--amber, oklch(0.62 0.14 70));
  margin-left: 4px;
}
.time-ago {
  font-size: 10px;
  color: var(--muted-foreground);
  flex-shrink: 0;
  white-space: nowrap;
}
.empty-hint {
  padding: 24px 14px;
  text-align: center;
  font-size: 11px;
  color: var(--muted-foreground);
}
</style>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useProjects } from '@/composables/useProjects'
import { useSessions, type SortOrder, type TimeRange } from '@/composables/useSessions'
import { useSessionMeta } from '@/composables/useSessionMeta'
import {
  displayTitle,
  relativeTime,
  formatTokens,
  formatBytes,
  tokenTotal,
  shortModel,
} from '@/types'
import type { SessionSummary } from '@/types'
import { useRunners } from '@/composables/useRunners'
import { resolveEnginePresentation } from '@/engines/presentation'
import WorkbenchTargetButton from '@/components/workbench/WorkbenchTargetButton.vue'
import TagChip from '@/components/archive/TagChip.vue'
import { useTagRegistry } from '@/composables/useTagRegistry'

const { t } = useI18n()
const { metaMap, getMeta, updateMeta } = useSessionMeta()
const { tags: registryTags, openManager } = useTagRegistry()

const { filteredSessions, sessionStats, loadProjects, engineOptions, selectedEngineIds, toggleEngine } = useProjects()
const {
  selectedSessionId,
  sortOrder,
  selectedTimeRange,
  selectedModel,
  selectedTags,
  starredOnly,
  filterAndSort,
  extractFilterOptions,
  selectSession,
  toggleTagFilter,
  clearTagFilters,
} = useSessions()
const sortedSessions = computed(() => filterAndSort(filteredSessions.value))
const filterOptions = computed(() => extractFilterOptions(filteredSessions.value))

const sortLabels = computed<Record<SortOrder, string>>(() => ({
  lastModified: t('archive.sortRecent'),
  tokenUsage: t('archive.sortTokens'),
  messageCount: t('archive.sortMessages'),
}))

const timeLabels = computed<Record<TimeRange, string>>(() => ({
  all: t('common.all'),
  today: t('archive.filterToday'),
  thisWeek: t('archive.filterWeek'),
  thisMonth: t('archive.filterMonth'),
}))

// 筛选下拉
const showModelDropdown = ref(false)
const showTagDropdown = ref(false)
const pendingStars = ref<Record<string, boolean>>({})

function engineBadgeClass(session: SessionSummary): string {
  const accent = resolveEnginePresentation(session.engine?.engineId, session.engine_name).accent
  if (accent === 'claude') return 'bg-claude/15 text-claude border-claude/30'
  if (accent === 'codex') return 'bg-codex/15 text-codex border-codex/30'
  return 'bg-primary/12 text-primary border-primary/25'
}

function engineFilterClass(engineId: string, active: boolean): string {
  const accent = resolveEnginePresentation(engineId.split('/')[0], null).accent
  if (accent === 'claude') return active
    ? 'bg-claude/15 text-claude border-claude/25'
    : 'text-claude/70 hover:text-claude hover:bg-claude/10'
  if (accent === 'codex') return active
    ? 'bg-codex/15 text-codex border-codex/25'
    : 'text-codex/70 hover:text-codex hover:bg-codex/10'
  return active
    ? 'bg-primary/15 text-primary border-primary/25'
    : 'text-muted-foreground hover:text-foreground'
}

function pickModel(model: string) {
  selectedModel.value = model
  showModelDropdown.value = false
}

// ====== 虚拟滚动（标题 / 标签 / 摘要四档组合高度） ======
const ITEM_H = 60
const ITEM_H_EXTRA = 20
const OVERSCAN = 5

function itemHeight(session: SessionSummary) {
  const meta = getMeta(session.id)
  return ITEM_H + (meta?.tags?.length ? ITEM_H_EXTRA : 0) + (meta?.summary ? ITEM_H_EXTRA : 0)
}

async function toggleSessionStar(event: MouseEvent, session: SessionSummary) {
  event.stopPropagation()
  const previous = !!getMeta(session.id)?.starred
  const next = !previous
  pendingStars.value = { ...pendingStars.value, [session.id]: next }
  metaMap.value = {
    ...metaMap.value,
    [session.id]: { ...metaMap.value[session.id], starred: next },
  }
  try {
    await updateMeta(session.id, { starred: next }, session.reference)
  } catch (cause) {
    metaMap.value = {
      ...metaMap.value,
      [session.id]: { ...metaMap.value[session.id], starred: previous },
    }
    useNotifications().notifyTransient(t('archive.starUpdateFailed'), String(cause))
  } finally {
    const pending = { ...pendingStars.value }
    delete pending[session.id]
    pendingStars.value = pending
  }
}

function sessionStarred(sessionId: string) {
  return pendingStars.value[sessionId] ?? !!getMeta(sessionId)?.starred
}

function pickTag(tag: string) {
  toggleTagFilter(tag)
}

const scrollContainer = ref<HTMLElement | null>(null)
const scrollTop = ref(0)
const containerHeight = ref(0)

const prefixHeights = computed(() => {
  const sessions = sortedSessions.value
  const h = new Float64Array(sessions.length + 1)
  for (let i = 0; i < sessions.length; i++)
    h[i + 1] = h[i] + itemHeight(sessions[i])
  return h
})

const totalHeight = computed(() => prefixHeights.value[sortedSessions.value.length])

function findIndex(top: number) {
  const h = prefixHeights.value
  let lo = 0, hi = h.length - 2
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1
    if (h[mid] <= top) lo = mid
    else hi = mid - 1
  }
  return lo
}

const visibleRange = computed(() => {
  const h = prefixHeights.value
  const len = sortedSessions.value.length
  const start = Math.max(0, findIndex(scrollTop.value) - OVERSCAN)
  let end = findIndex(scrollTop.value + containerHeight.value)
  end = Math.min(len, end + OVERSCAN + 1)
  return { start, end }
})

const visibleSessions = computed(() => {
  const { start, end } = visibleRange.value
  return sortedSessions.value.slice(start, end).map((session, i) => ({
    session,
    index: start + i,
    height: itemHeight(session),
  }))
})

const offsetY = computed(() => prefixHeights.value[visibleRange.value.start])

function onScroll() {
  const el = scrollContainer.value
  if (el) scrollTop.value = el.scrollTop
}

let resizeObserver: ResizeObserver | null = null

onMounted(() => {
  const el = scrollContainer.value
  if (el) {
    containerHeight.value = el.clientHeight
    resizeObserver = new ResizeObserver(() => {
      containerHeight.value = el.clientHeight
    })
    resizeObserver.observe(el)
  }
})

onUnmounted(() => {
  resizeObserver?.disconnect()
})

// 数据源变化时重置滚动
watch(sortedSessions, () => {
  const el = scrollContainer.value
  if (el && el.scrollTop > totalHeight.value) {
    el.scrollTop = 0
    scrollTop.value = 0
  }
})

// 原生右键菜单
import { invoke } from '@tauri-apps/api/core'
import { Menu } from '@tauri-apps/api/menu'
import { useWorkbench } from '@/composables/useWorkbench'
import { useConfirm } from '@/composables/useConfirm'
import { useNotifications } from '@/composables/useNotifications'
import { readStoredChannelId } from '@/composables/useSessionSettings'
import { resolveChannel, refreshChannels } from '@/composables/useChannels'

async function onContextMenu(e: MouseEvent, session: SessionSummary) {
  e.preventDefault()

  const items: Array<{ text: string; action: () => void }> = []

  if (session.cwd) {
    items.push({
      text: t('archive.resumeInTerminal'),
      action: async () => {
        await refreshChannels()
        const channel = resolveChannel(readStoredChannelId(session.id))
        try {
          await invoke('resume_in_terminal', { cwd: session.cwd!, sessionId: session.id, channel })
        } catch (err) {
          const { notifyTransient } = useNotifications()
          const denied = String(err).includes('AUTOMATION_DENIED')
          notifyTransient(
            denied ? t('topbar.automationDenied') : t('topbar.terminalFailed'),
            denied ? t('topbar.automationDeniedHint') : String(err),
          )
        }
      },
    })
  }

  items.push({
    text: t('archive.deleteSession'),
    action: async () => {
      const { projects } = useProjects()
      const project = projects.value.find(p => p.sessions.some(s => s.id === session.id))
      if (!project) return
      const { findSession, removeSession } = useWorkbench()
      const home = findSession(session.id)
      if (home) {
        const { confirm } = useConfirm()
        const ok = await confirm(
          t('archive.deleteSessionInWorkbench', { tabName: home.tab.name }),
          t('common.delete'),
        )
        if (!ok) return
        removeSession(session.id)
      }
      // 删除会话前先停止该会话下所有 runner（FR-007 生命周期治理）
      await useRunners().stopAllForSession(session.id)
      await invoke('delete_session', { projectId: project.id, sessionId: session.id })
      if (selectedSessionId.value === session.id) selectSession(null)
      loadProjects()
    },
  })

  const menu = await Menu.new({
    items: items.map(item => ({
      text: item.text,
      action: item.action,
    })),
  })
  await menu.popup()
}
</script>

<template>
  <div class="h-full flex flex-col">
    <!-- 统计卡片 -->
    <div class="px-3 py-2 flex items-center gap-1.5 whitespace-nowrap">
      <div class="flex-1 flex items-baseline gap-1 justify-center">
        <span class="text-sm font-semibold text-foreground">{{ sessionStats.sessionCount }}</span>
        <span class="text-xs text-muted-foreground">{{ $t('archive.sessionLabel') }}</span>
      </div>
      <span class="w-px h-3 bg-divider shrink-0" />
      <div class="flex-1 flex items-baseline gap-1 justify-center">
        <span class="text-sm font-semibold text-foreground">{{ formatTokens(sessionStats.totalTokens) }}</span>
        <span class="text-xs text-muted-foreground">Token</span>
      </div>
      <span class="w-px h-3 bg-divider shrink-0" />
      <div class="flex-1 flex items-baseline gap-1 justify-center">
        <span class="text-sm font-semibold text-foreground">{{ formatBytes(sessionStats.totalSize) }}</span>
        <span class="text-xs text-muted-foreground">{{ $t('archive.diskLabel') }}</span>
      </div>
    </div>

    <!-- 排序 -->
    <div class="px-3 py-1 flex items-center gap-2">
      <button
        v-for="(label, key) in sortLabels"
        :key="key"
        class="px-2 py-0.5 text-xs rounded transition-colors"
        :class="sortOrder === key ? 'bg-secondary text-foreground' : 'text-muted-foreground hover:text-foreground'"
        @click="sortOrder = key as SortOrder"
      >
        {{ label }}
      </button>
      <span class="flex-1" />
      <button
        class="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
        :title="$t('archive.refreshList')"
        @click="loadProjects"
      >
        <span class="i-carbon-renew w-3.5 h-3.5" />
      </button>
    </div>

    <!-- 筛选栏 -->
    <div class="px-3 py-1 flex flex-wrap gap-1.5 items-center">
      <button
        v-for="engine in engineOptions"
        :key="engine.id"
        class="px-2 py-0.5 text-xs rounded border border-transparent transition-colors"
        :class="engineFilterClass(engine.id, selectedEngineIds.has(engine.id))"
        @click="toggleEngine(engine.id)"
      >
        {{ engine.name }}
      </button>

      <span v-if="engineOptions.length" class="text-border">|</span>

      <!-- 时间范围 -->
      <button
        v-for="(label, key) in timeLabels"
        :key="key"
        class="px-2 py-0.5 text-xs rounded transition-colors"
        :class="selectedTimeRange === key ? 'bg-secondary text-foreground' : 'text-muted-foreground hover:text-foreground'"
        @click="selectedTimeRange = key as TimeRange"
      >
        {{ label }}
      </button>

      <span class="text-border">|</span>

      <!-- 模型下拉 -->
      <div class="relative">
        <button
          v-if="selectedModel"
          class="px-2 py-0.5 text-xs rounded bg-secondary text-foreground flex items-center gap-1"
          @click="selectedModel = null"
        >
          {{ selectedModel }} ×
        </button>
        <button
          v-else
          class="px-2 py-0.5 text-xs rounded text-muted-foreground hover:text-foreground flex items-center gap-0.5"
          @click.stop="showModelDropdown = !showModelDropdown"
        >
          {{ $t('archive.filterModel') }} <span class="i-carbon-chevron-down w-3 h-3" />
        </button>
        <div
          v-if="showModelDropdown && filterOptions.models.length"
          class="absolute top-full left-0 mt-1 z-10 bg-card border border-border rounded-md shadow-paper-lifted py-1 min-w-32 max-h-48 overflow-y-auto"
        >
          <button
            v-for="model in filterOptions.models"
            :key="model"
            class="w-full text-left px-3 py-1 text-xs hover:bg-muted text-muted-foreground truncate"
            @click="pickModel(model)"
          >
            {{ model }}
          </button>
        </div>
      </div>

      <!-- 星标 -->
      <button
        type="button"
        class="inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs transition-colors"
        :class="starredOnly ? 'bg-secondary text-foreground' : 'text-muted-foreground hover:text-foreground'"
        :aria-pressed="starredOnly"
        @click="starredOnly = !starredOnly"
      >
        <span class="h-3 w-3" :class="starredOnly ? 'i-carbon-star-filled' : 'i-carbon-star'" :style="starredOnly ? { color: 'var(--star)' } : undefined" />
        {{ $t('archive.starredOnly') }}
      </button>

      <!-- 标签下拉 -->
      <div class="relative">
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs transition-colors"
          :class="selectedTags.size ? 'bg-secondary text-foreground' : 'text-muted-foreground hover:text-foreground'"
          :aria-expanded="showTagDropdown"
          @click.stop="showTagDropdown = !showTagDropdown"
        >
          {{ $t('archive.tags') }}
          <span v-if="selectedTags.size" class="tabular-nums">{{ selectedTags.size }}</span>
          <span class="i-carbon-chevron-down h-3 w-3" />
        </button>
        <div v-if="showTagDropdown" class="absolute left-0 top-full z-20 mt-1 w-56 rounded-md border border-border bg-popover p-2 shadow-paper-lifted">
          <div class="flex max-h-48 flex-col gap-1 overflow-y-auto">
            <div
              v-for="tag in registryTags"
              :key="tag.name"
              class="flex min-w-0 items-center justify-between gap-2"
            >
              <TagChip
                :name="tag.name"
                :active="selectedTags.has(tag.name)"
                clickable
                compact
                @click="pickTag(tag.name)"
              />
              <span class="shrink-0 text-[9px] tabular-nums text-muted-foreground">{{ tag.usageCount }}</span>
            </div>
            <span v-if="!registryTags.length" class="px-1 py-2 text-[10px] text-muted-foreground">{{ $t('archive.noTags') }}</span>
          </div>
          <div class="mt-2 flex items-center justify-between border-t border-border pt-2">
            <button type="button" class="text-[10px] text-muted-foreground hover:text-foreground disabled:opacity-40" :disabled="!selectedTags.size" @click="clearTagFilters">{{ $t('common.clear') }}</button>
            <button type="button" class="text-[10px] text-primary hover:underline" @click="showTagDropdown = false; openManager()">{{ $t('archive.manageTags') }}</button>
          </div>
        </div>
      </div>
    </div>

    <!-- 会话列表（虚拟滚动） -->
    <div
      ref="scrollContainer"
      class="flex-1 overflow-y-auto min-h-0 overscroll-y-contain"
      @scroll.passive="onScroll"
    >
      <div v-if="sortedSessions.length === 0" class="px-3 py-8 text-center">
        <p class="text-muted-foreground text-xs">{{ $t('archive.noSessions') }}</p>
        <p class="text-muted-foreground text-xs mt-1">{{ $t('archive.adjustFilter') }}</p>
      </div>

      <div v-else :style="{ height: totalHeight + 'px', position: 'relative' }">
        <div :style="{ transform: `translateY(${offsetY}px)` }" class="p-2 flex flex-col gap-1">
          <template
            v-for="({ session, index, height }) in visibleSessions"
            :key="session.id"
          >
          <div v-if="index > 0" class="mx-3 border-t border-border/30" />
          <div
            class="w-full text-left px-3 py-2 rounded-md border border-transparent transition-colors cursor-pointer group relative shrink-0"
            :class="selectedSessionId === session.id ? 'bg-card border-border shadow-paper' : 'hover:bg-muted'"
            :style="{ height: height + 'px', boxSizing: 'border-box' }"
            @click="selectSession(session.id)"
            @contextmenu="onContextMenu($event, session)"
          >
            <div class="flex min-w-0 items-center gap-2">
              <div class="min-w-0 flex-1 truncate text-sm text-foreground">
                {{ displayTitle(session, getMeta(session.id)?.title) }}
              </div>
              <button
                type="button"
                class="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded text-muted-foreground opacity-0 transition-[opacity,color,background] hover:bg-muted group-hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                :class="sessionStarred(session.id) ? '!opacity-100' : ''"
                :style="sessionStarred(session.id) ? { color: 'var(--star)' } : undefined"
                :title="sessionStarred(session.id) ? $t('archive.unstar') : $t('archive.star')"
                :aria-pressed="sessionStarred(session.id)"
                :disabled="pendingStars[session.id] !== undefined"
                @click="toggleSessionStar($event, session)"
              >
                <span class="h-3.5 w-3.5" :class="sessionStarred(session.id) ? 'i-carbon-star-filled' : 'i-carbon-star'" />
              </button>
              <WorkbenchTargetButton :session-id="session.id" variant="secondary" compact />
            </div>
            <div class="text-xs text-muted-foreground mt-0.5 flex items-center gap-1.5 truncate">
              <span
                v-if="session.engine_name"
                class="inline-flex items-center gap-1 px-1.5 rounded border text-[9px] font-medium"
                :class="engineBadgeClass(session)"
              >
                <span class="w-1 h-1 rounded-full bg-current" />
                {{ session.engine_name }}
              </span>
              <span v-if="session.git_branch">{{ session.git_branch }}</span>
              <span v-if="session.git_branch">·</span>
              <span>{{ relativeTime(session.last_modified) }}</span>
              <span>·</span>
              <span>{{ formatTokens(tokenTotal(session.total_tokens)) }}</span>
              <span v-if="session.model">·</span>
              <span v-if="session.model" class="text-muted-foreground">{{ shortModel(session.model) }}</span>
            </div>
            <div v-if="getMeta(session.id)?.tags?.length" class="mt-1 flex min-w-0 items-center gap-1 overflow-hidden">
              <TagChip
                v-for="tag in getMeta(session.id)!.tags!.slice(0, 2)"
                :key="tag"
                :name="tag"
                :active="selectedTags.has(tag)"
                clickable
                compact
                @click.stop="pickTag(tag)"
              />
              <span v-if="getMeta(session.id)!.tags!.length > 2" class="shrink-0 text-[9px] text-muted-foreground">+{{ getMeta(session.id)!.tags!.length - 2 }}</span>
            </div>
            <!-- 摘要（仅展示） -->
            <div v-if="getMeta(session.id)?.summary" v-tooltip="getMeta(session.id)!.summary" class="mt-1 line-clamp-1 text-[11px] leading-relaxed text-muted-foreground/70">
              {{ getMeta(session.id)!.summary }}
            </div>
          </div>
          </template>
        </div>
      </div>
    </div>

  </div>
</template>

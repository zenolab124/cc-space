<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import WorkshopNav from '@/components/workshop/WorkshopNav.vue'
import AssetListColumn from '@/components/workshop/AssetListColumn.vue'
import McpListColumn from '@/components/workshop/McpListColumn.vue'
import AssetDetailPane from '@/components/workshop/AssetDetailPane.vue'
import McpDetailPane from '@/components/workshop/McpDetailPane.vue'
import McpAddDialog from '@/components/workshop/McpAddDialog.vue'
import HooksPane from '@/components/workshop/HooksPane.vue'
import MemoryPane from '@/components/workshop/MemoryPane.vue'
import { useWorkshop, mcpKey, type WorkshopCategory } from '@/composables/useWorkshop'
import { useHooks } from '@/composables/useHooks'
import { useMemory } from '@/composables/useMemory'
import { useUiState } from '@/composables/useUiState'
import type { WorkshopMcpServer } from '@/types'

/**
 * 工坊域（v2.9.0 三栏化）：WorkshopNav(190px) | 列表列(~300px) | 详情区(弹性)。
 * 六子导航：Skills / Commands / Subagents / MCP / Hooks / 记忆。
 * Hooks 和记忆子页本波走占位组件，后续同事填充。
 */

const { t } = useI18n()
const { activeSection } = useUiState()
const { assets, loading, error, ensureLoaded, refresh, retry, probeStates, probeMcpServers } = useWorkshop()
const { hooksCount, ensureLoaded: ensureHooksLoaded } = useHooks()
const { memoryCount, ensureLoaded: ensureMemoryLoaded, ensureFresh: refreshMemory } = useMemory()

/** 域内当前类别 */
const category = ref<WorkshopCategory>('skills')

/** 当前选中资产 path（切换类别时清空） */
const selectedPath = ref<string | null>(null)

/** 子导航配置（图标类名静态字面量，供 UnoCSS 扫描） */
const navItems: { key: WorkshopCategory; icon: string; label: string }[] = [
  { key: 'skills', icon: 'i-carbon-star', label: 'Skills' },
  { key: 'commands', icon: 'i-carbon-terminal', label: 'Commands' },
  { key: 'agents', icon: 'i-carbon-bot', label: 'Subagents' },
  { key: 'mcp', icon: 'i-carbon-plug', label: t('workshop.mcpServers') },
  { key: 'hooks', icon: 'i-carbon-events', label: t('workshop.hooks') },
  { key: 'memory', icon: 'i-carbon-time', label: t('workshop.memory') },
]

/** 子导航计数（hooks/memory 来自各自 module 单例，未加载显「…」） */
const counts = computed<Record<WorkshopCategory, string>>(() => {
  const a = assets.value
  const hooksN = hooksCount.value === null ? '…' : String(hooksCount.value)
  const memoryN = memoryCount.value === null ? '…' : String(memoryCount.value)
  if (!a) {
    const placeholder = error.value ? '—' : '…'
    return { skills: placeholder, commands: placeholder, agents: placeholder, mcp: placeholder, hooks: hooksN, memory: memoryN }
  }
  return {
    skills: String(a.skills.length),
    commands: String(a.commands.length),
    agents: String(a.agents.length),
    mcp: String(a.mcpServers.length),
    hooks: hooksN,
    memory: memoryN,
  }
})

/** 当前类别的列表项 */
const currentList = computed(() => {
  const a = assets.value
  switch (category.value) {
    case 'skills':
      return { icon: 'i-carbon-star', items: a?.skills ?? [], empty: t('workshop.noSkills'), hint: t('workshop.hintSkills') }
    case 'commands':
      return { icon: 'i-carbon-terminal', items: a?.commands ?? [], empty: t('workshop.noCommands'), hint: t('workshop.hintCommands') }
    case 'agents':
      return { icon: 'i-carbon-bot', items: a?.agents ?? [], empty: t('workshop.noSubagents'), hint: t('workshop.hintSubagents') }
    default:
      return null
  }
})

/** 选中项的元数据（name/version/source） */
const selectedMeta = computed(() => {
  const a = assets.value
  if (!selectedPath.value || !a) return null
  const allItems = [...a.skills, ...a.commands, ...a.agents]
  const item = allItems.find(i => i.path === selectedPath.value)
  if (!item) return null
  return {
    name: item.name,
    version: 'version' in item ? (item as { version?: string | null }).version : null,
    source: item.source,
  }
})

/** 列表头部配置 */
const headConfig: Record<string, { title: string; sub: string }> = {
  skills: { title: 'Skills', sub: t('workshop.subSkills') },
  commands: { title: 'Commands', sub: t('workshop.subCommands') },
  agents: { title: 'Subagents', sub: t('workshop.subAgents') },
  mcp: { title: t('workshop.mcpServers'), sub: t('workshop.subMcp') },
}
const head = computed(() => headConfig[category.value] ?? { title: '', sub: '' })

/** 是否是有列表列+详情区的标准三栏子页 */
const isThreeColumn = computed(() => {
  return ['skills', 'commands', 'agents', 'mcp'].includes(category.value)
})

/** 窗口响应式：<900px 时列表列收窄 */
const isNarrow = ref(false)
function checkWidth() {
  isNarrow.value = window.innerWidth < 900
}
if (typeof window !== 'undefined') {
  checkWidth()
  window.addEventListener('resize', checkWidth)
}

// 切换子导航类别时清空选中态（PRD FR-002）
watch(category, () => {
  selectedPath.value = null
})

// 首次进入工坊惰性加载（资产 + hooks/memory 计数三路都在此触发，进域即全亮，不提前到启动期）
watch(activeSection, (s) => {
  if (s === 'workshop') {
    ensureLoaded()
    ensureHooksLoaded()
    ensureMemoryLoaded()
  }
}, { immediate: true })

// 每次进入 MCP 子页重新探活
watch([activeSection, category], ([s, c]) => {
  if (s === 'workshop' && c === 'mcp') probeMcpServers()
})

// 每次进入记忆子页重扫（覆盖「切走再切回工坊」——此时 MemoryPane 不重新 mount）
watch([activeSection, category], ([s, c]) => {
  if (s === 'workshop' && c === 'memory') refreshMemory()
})

// 数据到达后若停留在 MCP 子页，补发探活
watch(assets, (a) => {
  if (a && activeSection.value === 'workshop' && category.value === 'mcp') probeMcpServers()
})

/** 当前选中的 MCP 服务器对象（详情面板用） */
const selectedMcpServer = computed<WorkshopMcpServer | null>(() => {
  if (category.value !== 'mcp' || !selectedPath.value || !assets.value) return null
  return assets.value.mcpServers.find(s => mcpKey(s) === selectedPath.value) ?? null
})

/** 添加 MCP 服务器弹窗 */
const mcpAddVisible = ref(false)

/** 「打开目录」失败的一次性文案 */
const openFailed = ref(false)
let openFailTimer: ReturnType<typeof setTimeout> | undefined

async function openDir() {
  try {
    await invoke('open_workshop_dir', { category: category.value })
  } catch (_) {
    openFailed.value = true
    clearTimeout(openFailTimer)
    openFailTimer = setTimeout(() => { openFailed.value = false }, 3000)
  }
}

function onSelect(path: string) {
  selectedPath.value = path
}

function onRefresh() {
  refresh()
  selectedPath.value = null
}

function onMcpAddSuccess() {
  mcpAddVisible.value = false
  onRefresh()
}
</script>

<template>
  <div class="h-full p-2.5">
    <div class="h-full flex bg-card border border-border rounded-lg shadow-paper overflow-hidden">
      <!-- 第一列：子导航 -->
      <WorkshopNav v-model="category" :items="navItems" :counts="counts" />

      <!-- Hooks/记忆子页：占位组件，不走三栏 -->
      <template v-if="category === 'hooks'">
        <div class="flex-1 min-w-0">
          <HooksPane />
        </div>
      </template>
      <template v-else-if="category === 'memory'">
        <div class="flex-1 min-w-0">
          <MemoryPane />
        </div>
      </template>

      <!-- 标准三栏：列表列 + 详情区 -->
      <template v-else>
        <!-- 第二列：列表列 -->
        <div class="col-list" :class="{ 'col-list-narrow': isNarrow }">
          <div class="list-head">
            <h2 class="list-title">{{ head.title }}</h2>
            <span class="list-sub">{{ head.sub }}</span>
          </div>
          <div class="list-toolbar">
            <span v-if="openFailed" class="text-xs text-destructive">{{ $t('common.openFailed') }}</span>
            <button v-if="category === 'mcp'" class="ws-btn ws-btn-primary" @click="mcpAddVisible = true">
              <span class="i-carbon-add w-3 h-3" />
              {{ $t('workshop.mcpAddBtn') }}
            </button>
            <button class="ws-btn" @click="openDir">{{ $t('workshop.openDir') }}</button>
            <button class="ws-btn" :disabled="loading" @click="onRefresh">
              <span class="i-carbon-renew w-3 h-3" :class="{ 'animate-spin': loading }" />
              {{ $t('common.refresh') }}
            </button>
          </div>

          <!-- 错误态 -->
          <div v-if="error" class="list-status">
            <p class="text-xs text-destructive">{{ $t('common.loadFailed') }}</p>
            <button class="ws-btn mt-2" @click="retry">{{ $t('common.retry') }}</button>
          </div>
          <!-- 加载态 -->
          <div v-else-if="!assets" class="list-status">
            <span class="text-xs text-muted-foreground">{{ $t('common.loading') }}</span>
          </div>
          <!-- 列表内容 -->
          <div v-else class="list-scroll">
            <AssetListColumn
              v-if="currentList"
              :items="currentList.items"
              :icon="currentList.icon"
              :selected-path="selectedPath"
              :empty-title="currentList.empty"
              :empty-hint="currentList.hint"
              :narrow="isNarrow"
              @select="onSelect"
            />
            <McpListColumn
              v-else
              :items="assets?.mcpServers ?? []"
              :probe-states="probeStates"
              :selected-path="selectedPath"
              :narrow="isNarrow"
              @select="onSelect"
            />
          </div>
        </div>

        <!-- 第三列：详情区 -->
        <div class="col-detail">
          <AssetDetailPane
            v-if="category !== 'mcp'"
            :path="selectedPath"
            :name="selectedMeta?.name"
            :version="selectedMeta?.version"
            :source="selectedMeta?.source"
            @refresh="onRefresh"
          />
          <!-- MCP 详情面板 -->
          <McpDetailPane
            v-else
            :server="selectedMcpServer"
            @refresh="onRefresh"
          />
        </div>
      </template>
    </div>

    <!-- MCP 添加服务器弹窗 -->
    <McpAddDialog
      :visible="mcpAddVisible"
      @close="mcpAddVisible = false"
      @success="onMcpAddSuccess"
    />
  </div>
</template>

<style scoped>
/* 列表列 */
.col-list {
  width: 300px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.col-list-narrow {
  width: 180px;
}
.list-head {
  padding: 12px 14px 8px;
  display: flex;
  align-items: center;
  gap: 8px;
  border-bottom: 1px solid var(--border);
}
.list-title {
  font-size: 13px;
  font-weight: 600;
  flex: 1;
}
.list-sub {
  font-size: 10.5px;
  color: var(--muted-foreground);
}
.list-toolbar {
  padding: 6px 14px;
  display: flex;
  align-items: center;
  gap: 6px;
  border-bottom: 1px solid var(--border);
}
.list-status {
  padding: 32px 14px;
  text-align: center;
}
.list-scroll {
  flex: 1;
  overflow-y: auto;
}

/* 详情区 */
.col-detail {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}
.detail-empty-wrap {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
}

/* 按钮 */
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
.ws-btn-primary {
  background: var(--primary);
  color: var(--primary-foreground);
  border-color: var(--primary);
}
.ws-btn-primary:hover:not(:disabled) {
  opacity: 0.9;
  box-shadow: none;
}
</style>

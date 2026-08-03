<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import TitleBar from '@/components/TitleBar.vue'
import TitleBarTools from '@/components/TitleBarTools.vue'
import ActivityBar from '@/components/ActivityBar.vue'
import WorkbenchTabs from '@/components/workbench/WorkbenchTabs.vue'
import SessionsView from '@/views/SessionsView.vue'
import SearchView from '@/views/SearchView.vue'
import WorkbenchView from '@/views/WorkbenchView.vue'
import WorkshopView from '@/views/WorkshopView.vue'
import AutomationView from '@/views/AutomationView.vue'
import SettingsView from '@/views/SettingsView.vue'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import PerfHud from '@/components/PerfHud.vue'
import { markBoot } from '@/utils/bootTrace'
import { useProjects } from '@/composables/useProjects'
import { useSessions } from '@/composables/useSessions'
import { useUiState } from '@/composables/useUiState'
import { initPermissionListener } from '@/composables/usePermissionRequests'
import { initStreamListeners } from '@/composables/useStreaming'
import { initTurnSignalListener } from '@/composables/useTurnSignals'
import { initNotificationLayer, useNotifications } from '@/composables/useNotifications'
import { useRoutines } from '@/composables/useRoutines'
import { initRunnerListeners } from '@/composables/useRunners'
import { initShortcuts } from '@/composables/useShortcuts'
import { migrateLegacyAppDefaults } from '@/composables/useChannels'
import { stateRepairCount, stateWasReset, useWorkbench } from '@/composables/useWorkbench'
import { applyZoom, useZoom } from '@/composables/useZoom'
import { useUpdater } from '@/composables/useUpdater'
import { initEngineRuntimeState } from '@/engines/runtimeState'
import { DragDropProvider, DragOverlay } from '@dnd-kit/vue'

const { projects, projectsRevision, loadProjects } = useProjects()
const { selectSession } = useSessions()
const { activeSection } = useUiState()
const { state, activeTab, pruneDrafts, reorderSessions, reorderColumns, expandSession } = useWorkbench()
const { t } = useI18n()
const { zoomLevel, setZoom, STEP } = useZoom()

// 草稿会话收割:projects 每次刷新后,把已落盘(或已被关闭弃用)的草稿清掉。
// watch 修订号而非 projects 本体:增量刷新原地 mutate 不换引用,浅层 watch 收不到
watch(projectsRevision, () => {
  const ids = new Set(projects.value.flatMap(p => p.sessions.map(s => s.id)))
  pruneDrafts(sid => ids.has(sid))
})

// 性能监视 HUD：v-if 懒挂载，关闭时零采样开销
const showPerfHud = ref(false)

function onKeydown(e: KeyboardEvent) {
  // Cmd+R: 刷新项目列表
  if ((e.metaKey || e.ctrlKey) && e.key === 'r') {
    e.preventDefault()
    loadProjects()
  }
  // Cmd+Shift+M: 性能监视 HUD（e.repeat 守卫：按住不连续 toggle）
  if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'm' || e.key === 'M') && !e.repeat) {
    e.preventDefault()
    showPerfHud.value = !showPerfHud.value
  }
  // Cmd+=/- : 缩放, Cmd+0 : 重置
  if (e.metaKey || e.ctrlKey) {
    if (e.key === '=' || e.key === '+') {
      e.preventDefault()
      setZoom(zoomLevel.value + STEP)
    } else if (e.key === '-') {
      e.preventDefault()
      setZoom(zoomLevel.value - STEP)
    } else if (e.key === '0') {
      e.preventDefault()
      setZoom(1)
    }
  }
  // Esc: 取消档案馆选择
  if (e.key === 'Escape' && activeSection.value === 'sessions') {
    selectSession(null)
  }
}

// --- 拖拽实时排序 ---
const draggingSessionId = ref<string | null>(null)
const ghostHtml = ref('')

function onBeforeDragStart(event: any) {
  const sourceId = String(event.operation?.source?.id ?? '')
  if (sourceId.startsWith('session:')) {
    draggingSessionId.value = sourceId.slice(8)
    const el = event.operation?.source?.element
    if (el instanceof HTMLElement) {
      ghostHtml.value = el.outerHTML
    }
  }
}

function onWorkbenchDragOver(event: any) {
  const source = event.operation?.source
  const target = event.operation?.target
  if (!source || !target) return
  const sourceId = String(source.id ?? '')
  const targetId = String(target.id ?? '')
  if (sourceId.startsWith('session:') && targetId.startsWith('session-drop:')) {
    const fromSid = sourceId.slice(8)
    const toSid = targetId.slice(13)
    if (fromSid === toSid) return
    const tab = activeTab.value
    const fromIdx = tab.sessionIds.indexOf(fromSid)
    const toIdx = tab.sessionIds.indexOf(toSid)
    if (fromIdx >= 0 && toIdx >= 0 && fromIdx !== toIdx) {
      reorderSessions(tab.id, fromIdx, toIdx)
    }
  }
}

function onWorkbenchDragEnd(event: any) {
  draggingSessionId.value = null
  ghostHtml.value = ''
  if (event.canceled) return
  const source = event.operation?.source
  const target = event.operation?.target
  if (!source || !target) return

  const sourceId = String(source.id ?? '')
  const targetId = String(target.id ?? '')
  if (import.meta.env.DEV) console.log('[dnd-end]', sourceId, '→', targetId)

  // Column reorder (both start with "col:")
  if (sourceId.startsWith('col:') && targetId.startsWith('col:')) {
    // col:{tabId}:{index}
    const [, tabId, fromStr] = sourceId.split(':')
    const [, , toStr] = targetId.split(':')
    const from = parseInt(fromStr)
    const to = parseInt(toStr)
    if (!isNaN(from) && !isNaN(to) && from !== to) {
      reorderColumns(tabId, from, to > from ? to : to)
    }
    return
  }

  // Session dropped on column area
  if (sourceId.startsWith('session:') && targetId.startsWith('col-zone:')) {
    const tabId = targetId.slice(9)
    const sessionId = sourceId.slice(8)
    expandSession(tabId, sessionId)
    return
  }
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  // 全局事件监听:整个 app 生命周期各注册一次
  await initStreamListeners()
  await initTurnSignalListener()
  await initPermissionListener()
  await initEngineRuntimeState()
  await initNotificationLayer()
  await initShortcuts()
  await useRoutines().initRoutineListener()
  await initRunnerListeners()
  applyZoom()
  // 旧「应用默认思考强度」一次性迁移进 official 渠道默认(不阻塞启动)
  migrateLegacyAppDefaults()
  // 档案馆预加载:v-show 保活但数据要提前拉，首次切换零等待
  // 计时（overlay，不改变 fire-and-forget 不阻塞 onMounted 的行为）：
  // 调用前打 boot:projects-start，promise settle 后打 boot:projects-done。
  markBoot('boot:projects-start')
  loadProjects().finally(() => markBoot('boot:projects-done'))
  // 工作台持久化损坏回退提示(NFR-002);可局部修复时只提示不重置
  if (stateWasReset) {
    useNotifications().notifyTransient(t('workbench.stateReset'))
  } else if (stateRepairCount > 0) {
    useNotifications().notifyTransient(t('workbench.stateRepaired', { count: stateRepairCount }))
  }
  useUpdater().initAutoCheck()
  markBoot('boot:app-ready')
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div class="h-screen w-screen flex flex-col bg-background text-foreground" @contextmenu.prevent>
    <DragDropProvider @before-drag-start="onBeforeDragStart" @drag-over="onWorkbenchDragOver" @drag-end="onWorkbenchDragEnd">
      <TitleBar>
        <template #leading>
          <WorkbenchTabs v-if="activeSection === 'workbench'" />
        </template>
        <template #trailing>
          <TitleBarTools />
        </template>
      </TitleBar>
      <div class="flex-1 flex min-h-0">
        <ActivityBar />
        <WorkbenchView v-show="activeSection === 'workbench'" class="flex-1 min-w-0" />
        <SessionsView v-show="activeSection === 'sessions'" class="flex-1 min-w-0" />
        <SearchView v-show="activeSection === 'search'" class="flex-1 min-w-0" />
        <WorkshopView v-show="activeSection === 'workshop'" class="flex-1 min-w-0" />
        <AutomationView v-show="activeSection === 'automation'" class="flex-1 min-w-0" />
        <SettingsView v-show="activeSection === 'settings'" class="flex-1 min-w-0" />
      </div>

      <DragOverlay :disabled="!draggingSessionId">
        <div v-if="ghostHtml" v-html="ghostHtml" class="pointer-events-none opacity-85" />
      </DragOverlay>

      <ConfirmDialog />
      <PerfHud v-if="showPerfHud" @close="showPerfHud = false" />
    </DragDropProvider>
  </div>
</template>

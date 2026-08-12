<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-dialog'
import { useWorkbench } from '@/composables/useWorkbench'
import { useProjects } from '@/composables/useProjects'
import { useUiState } from '@/composables/useUiState'
import { useNotifications } from '@/composables/useNotifications'
import { fileName } from '@/utils/path'
import MonitorCard from './MonitorCard.vue'
import type { Project } from '@/types'

const { t } = useI18n()
const { activeTab, createPendingTask } = useWorkbench()
const { projects } = useProjects()
const { switchSection } = useUiState()
const { notifyTransient } = useNotifications()

const expandedSet = computed(() => new Set(activeTab.value.columns.map(c => c.sessionId)))

const hint = computed(() => {
  const n = activeTab.value.sessionIds.length
  const m = activeTab.value.columns.length
  return n === 0 ? null : t('workbench.rail.hint', { n, m })
})

const showPopover = ref(false)
const creating = ref(false)
const popoverRef = ref<HTMLElement>()
const triggerRef = ref<HTMLButtonElement>()

interface ProjectOption {
  id: string
  cwd: string
  lastActive: number
}

/** 双引擎项目按工作目录合并；同目录只保留最近活跃的一项。 */
const projectOptions = computed<ProjectOption[]>(() => {
  const options = new Map<string, ProjectOption>()
  for (const project of projects.value) {
    const cwd = projectCwd(project)
    if (!cwd) continue
    const key = cwd.replace(/[\\/]+$/, '')
    const lastActive = project.last_active ?? 0
    const current = options.get(key)
    if (!current || lastActive > current.lastActive) {
      options.set(key, { id: key, cwd, lastActive })
    }
  }
  return [...options.values()].sort((a, b) => b.lastActive - a.lastActive)
})

const recentProjects = computed(() => projectOptions.value.slice(0, 5))
// 二级面板只列一级没展示的,不重复
const moreProjects = computed(() => projectOptions.value.slice(5))
const hasMore = computed(() => moreProjects.value.length > 0)
const showAllProjects = ref(false)
const subMenuRef = ref<HTMLElement>()
const subMenuStyle = ref<Record<string, string>>({})
let subMenuLeaveTimer = 0

function projectCwd(project: Project): string | null {
  return project.sessions.find(session => session.cwd)?.cwd ?? project.source_path ?? null
}

function cancelSubMenuClose() {
  clearTimeout(subMenuLeaveTimer)
}

function scheduleSubMenuClose() {
  cancelSubMenuClose()
  subMenuLeaveTimer = window.setTimeout(() => {
    showAllProjects.value = false
  }, 150)
}

async function positionSubMenu() {
  cancelSubMenuClose()
  subMenuStyle.value = {}
  showAllProjects.value = true
  await nextTick()
  const el = subMenuRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const style: Record<string, string> = {}

  if (rect.right > vw) {
    style.left = 'auto'
    style.right = '100%'
    style.marginRight = '4px'
    style.marginLeft = '0'
  }

  if (rect.bottom > vh) {
    style.top = 'auto'
    style.bottom = '0'
  }

  subMenuStyle.value = style
}

function togglePopover() {
  showPopover.value = !showPopover.value
}

function closePopover() {
  showPopover.value = false
  showAllProjects.value = false
  subMenuStyle.value = {}
}

function onDocumentClick(e: MouseEvent) {
  if (!showPopover.value) return
  const target = e.target as Node
  if (popoverRef.value && !popoverRef.value.contains(target)
    && triggerRef.value && !triggerRef.value.contains(target)) {
    closePopover()
  }
}

function selectProject(project: ProjectOption) {
  if (creating.value) return
  createPendingTask(project.cwd)
  closePopover()
}

function goToArchive() {
  switchSection('sessions')
  closePopover()
}

async function pickFolder() {
  closePopover()
  creating.value = true
  try {
    const selected = await open({ directory: true, multiple: false })
    if (selected) createPendingTask(selected)
  } catch (cause) {
    notifyTransient(t('common.openFailed'), String(cause))
  } finally {
    creating.value = false
  }
}

onMounted(() => {
  document.addEventListener('mousedown', onDocumentClick)
})
onUnmounted(() => document.removeEventListener('mousedown', onDocumentClick))
</script>

<template>
  <aside class="w-64 shrink-0 border-r border-border p-2.5 flex flex-col gap-2 min-h-0">
    <div v-if="hint" class="px-0.5 text-[10.5px] text-muted-foreground shrink-0">{{ hint }}</div>

    <!-- px/py + 等量负 margin:overflow-y-auto 会连带裁掉 X 轴溢出,
         给卡片阴影留画布,视觉布局不变 -->
    <TransitionGroup
      tag="div"
      name="card"
      class="flex-1 min-h-0 overflow-y-auto flex flex-col gap-2 px-2 -mx-2 py-2 -my-2"
    >
      <MonitorCard
        v-for="sid in activeTab.sessionIds"
        :key="sid"
        class="shrink-0"
        :session-id="sid"
        :expanded="expandedSet.has(sid)"
      />

      <div
        v-if="activeTab.sessionIds.length === 0"
        key="empty-state"
        class="px-2 py-6 text-center text-xs text-muted-foreground leading-relaxed"
      >
        {{ $t('workbench.rail.empty') }}
      </div>
    </TransitionGroup>

    <!-- ＋ 格 + Popover -->
    <div class="relative shrink-0">
      <!-- Popover 面板（向上弹出） -->
      <Transition name="popover">
        <div
          v-if="showPopover"
          ref="popoverRef"
          class="absolute bottom-full left-0 right-0 mb-1.5 z-50
                 rounded border border-border shadow-paper-lifted bg-popover"
        >
          <!-- 最近项目 -->
          <div v-if="recentProjects.length" class="py-1">
            <div class="px-2.5 py-1 text-[10px] text-muted-foreground/70 tracking-wider">
              {{ $t('workbench.rail.recentProjects') }}
            </div>
            <button
              v-for="p in recentProjects"
              :key="p.id"
              class="w-full px-2.5 py-1.5 text-left text-xs text-muted-foreground
                     hover:bg-muted hover:text-foreground transition-colors
                     flex items-center gap-2"
              :disabled="creating"
              @click="selectProject(p)"
            >
              <span class="i-carbon-folder w-3.5 h-3.5 shrink-0 opacity-60" />
              <span class="truncate">{{ fileName(p.cwd) }}</span>
            </button>

            <!-- 更多项目（二级,去重:只列一级之外的） -->
            <div
              v-if="hasMore"
              class="relative"
              @mouseenter="positionSubMenu"
              @mouseleave="scheduleSubMenuClose"
            >
              <button
                class="w-full px-2.5 py-1.5 text-left text-xs text-muted-foreground
                       hover:bg-muted hover:text-foreground transition-colors
                       flex items-center gap-2"
              >
                <span class="i-carbon-list w-3.5 h-3.5 shrink-0 opacity-60" />
                <span class="flex-1">{{ $t('workbench.rail.moreProjects') }}</span>
                <span class="i-carbon-chevron-right w-3 h-3 opacity-40" />
              </button>

              <!-- 二级面板 -->
              <div
                v-if="showAllProjects"
                ref="subMenuRef"
                :style="subMenuStyle"
                class="absolute left-full top-0 ml-1 z-50 w-56
                       rounded border border-border shadow-paper-lifted bg-popover
                       py-1 max-h-96 overflow-y-auto"
              >
                <button
                  v-for="p in moreProjects"
                  :key="p.id"
                  class="w-full px-2.5 py-1.5 text-left text-muted-foreground
                         hover:bg-muted hover:text-foreground transition-colors
                         flex items-center gap-2"
                  :disabled="creating"
                  @click="selectProject(p)"
                >
                  <span class="i-carbon-folder w-3.5 h-3.5 shrink-0 opacity-60 mt-0.5 self-start" />
                  <span class="min-w-0">
                    <span class="block text-xs truncate">{{ fileName(p.cwd) }}</span>
                    <span class="block text-[10px] text-muted-foreground/50 truncate">{{ p.cwd }}</span>
                  </span>
                </button>
              </div>
            </div>
          </div>

          <!-- 分隔线 -->
          <div class="border-t border-border" />

          <!-- 固定操作 -->
          <div class="py-1">
            <button
              class="w-full px-2.5 py-1.5 text-left text-xs text-muted-foreground
                     hover:bg-muted hover:text-foreground transition-colors
                     flex items-center gap-2"
              @click="goToArchive"
            >
              <span class="i-carbon-archive w-3.5 h-3.5 shrink-0 opacity-60" />
              <span>{{ $t('workbench.rail.fromArchive') }}</span>
            </button>
            <button
              class="w-full px-2.5 py-1.5 text-left text-xs text-muted-foreground
                     hover:bg-muted hover:text-foreground transition-colors
                     flex items-center gap-2"
              @click="pickFolder"
              :disabled="creating"
            >
              <span class="i-carbon-folder-add w-3.5 h-3.5 shrink-0 opacity-60" />
              <span>{{ $t('workbench.rail.pickFolder') }}</span>
            </button>
          </div>
        </div>
      </Transition>

      <button
        ref="triggerRef"
        class="w-full min-h-10 border border-dashed border-border rounded text-xs text-muted-foreground
               flex items-center justify-center gap-1.5 hover:text-primary hover:border-primary transition-colors"
        @click="togglePopover"
      >
        <span class="text-sm">＋</span>
        <span>{{ $t('workbench.rail.openSession') }}</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.card-move {
  transition: transform 0.2s ease;
}
.card-leave-active {
  position: absolute;
  opacity: 0;
}

.popover-enter-active,
.popover-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}
.popover-enter-from,
.popover-leave-to {
  opacity: 0;
  transform: translateY(4px);
}
</style>

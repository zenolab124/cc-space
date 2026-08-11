<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDroppable } from '@dnd-kit/vue'
import { useWorkbench, setRightZoneWidth } from '@/composables/useWorkbench'
import { inheritRunSettings } from '@/composables/useSessionSettings'
import { useProjects } from '@/composables/useProjects'
import { useNotifications } from '@/composables/useNotifications'
import { useHorizontalWheelScroll } from '@/composables/useHorizontalWheelScroll'
import { useColumnResize } from '@/composables/useColumnResize'
import { createSession, forkSession } from '@/engines/client'
import { sessionUiId, usesNativeSessionSurface } from '@/engines/integration'
import { engineRuntimeChannel, engineRuntimeOptions, inheritEngineRunConfig } from '@/engines/runConfig'
import WorkbenchColumnView from './WorkbenchColumn.vue'
import SortableColumn from './SortableColumn.vue'

const {
  activeTab,
  expandSession,
  reorderColumns,
  focusColumnRequest,
  createRaceTab,
  draftCwd,
  registerFork,
  engineDraft,
  stageEngineDraft,
  suppressColumnTransition,
} = useWorkbench()
const { t } = useI18n()
const { projects } = useProjects()
const { notifyTransient } = useNotifications()

function resolveSessionCwd(sessionId: string): string | undefined {
  for (const p of projects.value) {
    const s = p.sessions.find(s => s.id === sessionId)
    if (s?.cwd) return s.cwd
  }
  return draftCwd(sessionId) ?? engineDraft(sessionId)?.cwd ?? undefined
}

const raceStarting = new Set<string>()

async function onStartRace(sessionId: string) {
  if (raceStarting.has(sessionId)) return
  raceStarting.add(sessionId)
  try {
    const session = projects.value
      .flatMap(project => project.sessions)
      .find(item => item.id === sessionId)
    const draft = engineDraft(sessionId)
    const cwd = resolveSessionCwd(sessionId)
    if (!cwd) {
      notifyTransient(t('workbench.race.noCwd'))
      return
    }
    const reference = session?.reference ?? draft?.reference
    const project = session?.project_reference ?? draft?.project
    if (reference && !usesNativeSessionSurface(reference.engine)) {
      if (!project) {
        notifyTransient(t('common.forkSessionFailed'), t('common.runtimeUnavailable'))
        return
      }
      const runtimeDraft = !!draft && !session
      const attachedChannel = runtimeDraft
        ? draft.attachedChannel
        : engineRuntimeChannel(sessionId)
      const options = {
        ...engineRuntimeOptions(sessionId),
        ...(attachedChannel ? { channelId: attachedChannel } : {}),
      }
      // Codex 的 thread/fork 只能读取已落盘线程；首条消息前的内存草稿改为同源新建。
      const created = runtimeDraft
        ? await createSession(project, cwd, options)
        : await forkSession(reference, null, options)
      const forkedSessionId = sessionUiId(created.session)
      stageEngineDraft(forkedSessionId, {
        reference: created.session,
        project,
        engineName: session?.engine_name ?? draft?.engineName ?? reference.engine.engineId,
        cwd,
        attachedChannel,
      })
      inheritEngineRunConfig(sessionId, forkedSessionId)
      createRaceTab(sessionId, cwd, forkedSessionId)
      return
    }
    const newSessionId = crypto.randomUUID()
    // 无条件登记分叉意图:源有无历史由 Rust 端按源 jsonl 真值判决(未落盘则退化新建)。
    // 前端 drafts 的收割是异步滞后的,不能当"源无历史"的判据
    registerFork(newSessionId, sessionId, cwd)
    inheritRunSettings(sessionId, newSessionId)
    createRaceTab(sessionId, cwd, newSessionId)
  } catch (cause) {
    notifyTransient(t('common.forkSessionFailed'), String(cause))
  } finally {
    raceStarting.delete(sessionId)
  }
}

const containerRef = ref<HTMLElement>()
const { isDropTarget } = useDroppable({ id: computed(() => 'col-zone:' + activeTab.value.id), element: containerRef })

let zoneResizeObserver: ResizeObserver | null = null

useHorizontalWheelScroll(containerRef)

onMounted(() => {
  const el = containerRef.value
  if (!el) return
  setRightZoneWidth(el.clientWidth)
  zoneResizeObserver = new ResizeObserver((entries) => {
    for (const entry of entries) {
      if (entry.contentRect.width > 0) {
        setRightZoneWidth(entry.contentRect.width)
      }
    }
  })
  zoneResizeObserver.observe(el)
})

onUnmounted(() => {
  zoneResizeObserver?.disconnect()
  zoneResizeObserver = null
})

const { dragging, shiftDragging, onDividerMouseDown } = useColumnResize()

// --- 幂等展开的滚动聚焦(FR-003:点击已展开卡 → 聚焦该列) ---

const flashIndex = ref(-1)

watch(focusColumnRequest, async (req) => {
  if (!req) return
  const idx = activeTab.value.columns.findIndex(c => c.sessionId === req.sessionId)
  if (idx < 0) return
  await nextTick()
  const colEl = containerRef.value?.querySelectorAll('.sortable-col')[idx] as HTMLElement | undefined
  colEl?.scrollIntoView({ behavior: 'smooth', inline: 'nearest', block: 'nearest' })
  flashIndex.value = idx
  window.setTimeout(() => {
    flashIndex.value = -1
  }, 900)
})
</script>

<template>
  <div class="flex-1 min-w-0 h-full relative">
    <div
      ref="containerRef"
      class="h-full flex flex-row p-2.5 gap-2.5 overflow-x-auto"
      :class="{ 'drop-target-highlight': isDropTarget }"
    >
      <!-- 空态(FR-004) -->
      <div
        v-if="activeTab.columns.length === 0"
        class="flex-1 grid place-items-center text-xs text-muted-foreground"
      >
        {{ $t('workbench.columns.empty') }}
      </div>

      <SortableColumn
        v-for="(col, i) in activeTab.columns"
        :key="col.id"
        :tab-id="activeTab.id"
        :index="i"
        :flex="activeTab.columnSizes[i]"
        :resizing="dragging || suppressColumnTransition"
      >
        <template #default="{ isDragging: colDragging, handleRef }">
          <WorkbenchColumnView :column="col" :tab-id="activeTab.id" :index="i" :dragging="colDragging" :handle-ref="handleRef" @start-race="onStartRace(col.sessionId)" />
          <!-- 列右边缘 resize 手柄（绝对定位，不参与 flex 布局） -->
          <div
            class="absolute top-0 bottom-0 -right-[7px] w-[14px] cursor-col-resize z-20"
            :class="{ 'divider-shift': shiftDragging }"
            @pointerdown.stop
            @mousedown="onDividerMouseDown($event, i)"
          />
        </template>
      </SortableColumn>
    </div>
  </div>
</template>

<style scoped>
.drop-target-highlight {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
  border-radius: 6px;
}
.divider-shift {
  background: color-mix(in srgb, var(--primary) 25%, transparent);
}
</style>

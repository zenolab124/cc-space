<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue'
import ProjectSidebar from '@/components/ProjectSidebar.vue'
import SessionList from '@/components/SessionList.vue'
import TagManagerDialog from '@/components/archive/TagManagerDialog.vue'
import UnifiedSessionDetail from '@/components/session/UnifiedSessionDetail.vue'
import { useUiState } from '@/composables/useUiState'
import { useProjects } from '@/composables/useProjects'
import { useSessions } from '@/composables/useSessions'

const { sidebarsCollapsed, projectSidebarWidth, sessionListWidth } = useUiState()
const { projects } = useProjects()
const { selectedSessionId } = useSessions()
const selectedSession = computed(() => projects.value
  .flatMap(project => project.sessions)
  .find(session => session.id === selectedSessionId.value) ?? null)

const isResizing = ref(false)

const displayProjectWidth = computed(() =>
  sidebarsCollapsed.value ? '0px' : `${projectSidebarWidth.value}px`,
)
const displaySessionListWidth = computed(() =>
  sidebarsCollapsed.value ? '0px' : `${sessionListWidth.value}px`,
)

let resizeTarget: 'project' | 'sessionList' | null = null
let startX = 0
let startWidth = 0

function preventDefault(e: Event) { e.preventDefault() }

function startResize(target: 'project' | 'sessionList', e: MouseEvent) {
  e.preventDefault()
  resizeTarget = target
  startX = e.clientX
  startWidth = target === 'project' ? projectSidebarWidth.value : sessionListWidth.value
  isResizing.value = true
  document.addEventListener('mousemove', onMouseMove)
  document.addEventListener('mouseup', onMouseUp)
  document.addEventListener('selectstart', preventDefault)
  document.body.style.cursor = 'col-resize'
}

function onMouseMove(e: MouseEvent) {
  if (!resizeTarget) return
  const delta = e.clientX - startX
  const min = resizeTarget === 'project' ? 224 : 288
  const w = Math.max(min, startWidth + delta)
  if (resizeTarget === 'project') projectSidebarWidth.value = w
  else sessionListWidth.value = w
}

function onMouseUp() {
  resizeTarget = null
  isResizing.value = false
  document.removeEventListener('mousemove', onMouseMove)
  document.removeEventListener('mouseup', onMouseUp)
  document.removeEventListener('selectstart', preventDefault)
  document.body.style.cursor = ''
}

onUnmounted(() => {
  document.removeEventListener('mousemove', onMouseMove)
  document.removeEventListener('mouseup', onMouseUp)
  document.removeEventListener('selectstart', preventDefault)
})
</script>

<template>
  <div class="h-full flex min-h-0">
    <aside
      class="shrink-0 overflow-hidden"
      :class="{ 'sidebar-collapsible': !isResizing }"
      :style="{ width: displayProjectWidth }"
    >
      <ProjectSidebar />
    </aside>

    <div
      v-if="!sidebarsCollapsed"
      class="shrink-0 w-1 border-l border-border cursor-col-resize hover:bg-primary/10 active:bg-primary/20 transition-colors"
      @mousedown="startResize('project', $event)"
    />

    <section
      class="shrink-0 overflow-hidden"
      :class="{ 'sidebar-collapsible': !isResizing }"
      :style="{ width: displaySessionListWidth }"
    >
      <SessionList />
    </section>

    <div
      v-if="!sidebarsCollapsed"
      class="shrink-0 w-1 border-l border-border cursor-col-resize hover:bg-primary/10 active:bg-primary/20 transition-colors"
      @mousedown="startResize('sessionList', $event)"
    />

    <main class="flex-1 min-w-0 p-2.5">
      <div class="h-full bg-card border border-border rounded shadow-paper overflow-hidden">
        <UnifiedSessionDetail :session="selectedSession" :session-id="selectedSessionId" mode="archive" />
      </div>
    </main>
    <TagManagerDialog />
  </div>
</template>

<style scoped>
.sidebar-collapsible {
  transition: width 220ms cubic-bezier(0.32, 0.72, 0, 1);
}
</style>

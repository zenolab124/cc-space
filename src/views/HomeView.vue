<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import TokenCard from '../components/home/TokenCard.vue'
import ModelPreferenceCard from '../components/home/ModelPreferenceCard.vue'
import WorkRhythmCard from '../components/home/WorkRhythmCard.vue'
import SessionDepthCard from '../components/home/SessionDepthCard.vue'
import ProjectActivityCard from '../components/home/ProjectActivityCard.vue'
import HeatmapCard from '../components/home/HeatmapCard.vue'
import { useHomeStats } from '../composables/useHomeStats'
import { useProjects } from '../composables/useProjects'
import { useSessions } from '../composables/useSessions'
import { useUiState } from '../composables/useUiState'

const { t } = useI18n()
const { activeSection, switchSection } = useUiState()
const {
  usage, usageLoading, usageError, retryUsage,
  ensureLoaded,
} = useHomeStats()
const { projects, loading: projectsLoading, loadProjects } = useProjects()
const { selectSession } = useSessions()

// 首页已屏蔽,待后续重新设计时恢复
// watch(activeSection, (section) => { if (section === 'home') { ensureLoaded(); loadProjects() } }, { immediate: true })

const headDate = computed(() => {
  const d = new Date()
  return t('time.dateHeader', { year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate() })
})

function onSelectDate(date: string) {
  void date
  switchSection('sessions')
}
</script>

<template>
  <main class="h-full overflow-y-auto px-8 py-6.5">
    <div class="content-area">
      <div class="flex items-center gap-3 mb-5">
        <span class="text-xs text-muted-foreground">{{ headDate }}</span>
      </div>

      <div class="dashboard-grid">
        <div class="span-full">
          <TokenCard :usage="usage" :loading="usageLoading" :error="usageError" @retry="retryUsage" />
        </div>

        <ModelPreferenceCard :projects="projects" :loading="projectsLoading" />
        <WorkRhythmCard :projects="projects" :loading="projectsLoading" />

        <div class="span-full">
          <HeatmapCard :usage="usage" :loading="usageLoading" :error="usageError" @retry="retryUsage" @select-date="onSelectDate" />
        </div>

        <ProjectActivityCard :projects="projects" :loading="projectsLoading" />
        <SessionDepthCard :projects="projects" :loading="projectsLoading" />
      </div>
    </div>
  </main>
</template>

<style scoped>
.dashboard-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
}
.span-full {
  grid-column: 1 / -1;
}
</style>

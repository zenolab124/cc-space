<script setup lang="ts">
import { computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
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
import { isMac } from '../composables/usePlatform'

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

// macOS WidgetKit 数据同步（保留）
watch(
  [usage, projects],
  async ([u, p]) => {
    if (!isMac || !u || !p.length) return
    const cfg = await invoke<{ dayStartHour: number }>('get_widget_config').catch(() => ({ dayStartHour: 0 }))
    const now = new Date()
    let startTs: number
    let dateKey: string
    if (cfg.dayStartHour < 0) {
      startTs = (now.getTime() - 86400000) / 1000
      dateKey = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
    } else {
      const h = cfg.dayStartHour
      const boundary = new Date(now.getFullYear(), now.getMonth(), now.getDate(), h)
      if (now.getTime() < boundary.getTime()) boundary.setDate(boundary.getDate() - 1)
      startTs = boundary.getTime() / 1000
      dateKey = `${boundary.getFullYear()}-${String(boundary.getMonth() + 1).padStart(2, '0')}-${String(boundary.getDate()).padStart(2, '0')}`
    }
    let todayTokens = 0
    if (cfg.dayStartHour < 0) {
      const yesterday = new Date(now.getTime() - 86400000)
      const yd = `${yesterday.getFullYear()}-${String(yesterday.getMonth() + 1).padStart(2, '0')}-${String(yesterday.getDate()).padStart(2, '0')}`
      todayTokens = (u.daily.find(d => d.date === dateKey)?.total ?? 0) + (u.daily.find(d => d.date === yd)?.total ?? 0)
    } else {
      todayTokens = u.daily.find(d => d.date === dateKey)?.total ?? 0
    }
    let sessions = 0
    for (const proj of p) {
      for (const s of proj.sessions) {
        if (s.last_modified >= startTs) sessions++
      }
    }
    const models = u.month.byModel.map(m => m.model)
    invoke('update_widget', { todaySessions: sessions, todayTokens: todayTokens, models })
      .catch(error => console.warn('[Monet] widget data sync failed', error))
  },
)

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

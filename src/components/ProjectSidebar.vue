<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useProjects } from '@/composables/useProjects'
import { formatBytes, formatTokens, tokenTotal, relativeTime } from '@/types'
import { fileName, parentPath, abbreviateHome } from '@/utils/path'

const { t } = useI18n()
const {
  archiveProjects,
  selectedProjectIds,
  loading,
  error,
  loadProjects,
  toggleProject,
  sidebarStats,
} = useProjects()

function isSelected(id: string) {
  return selectedProjectIds.value.has(id)
}

function projectName(displayPath: string) {
  return fileName(displayPath)
}

function projectPath(displayPath: string) {
  const parent = parentPath(displayPath)
  if (!parent) return ''
  const raw = abbreviateHome(parent)
  if (raw === '~' || raw.split(/[/\\]/).filter(Boolean).length < 2) return ''
  return raw
}

function projectTokens(project: typeof archiveProjects.value[number]) {
  return project.sessions.reduce((sum, s) => sum + tokenTotal(s.total_tokens), 0)
}

</script>

<template>
  <div class="h-full flex flex-col" @contextmenu.prevent>
    <!-- 标题 -->
    <div class="px-3 py-2 text-xs font-semibold text-muted-foreground tracking-wide">
      {{ $t('archive.projects') }}
    </div>

    <!-- 加载态 -->
    <div v-if="loading" class="px-3 py-8 text-center text-muted-foreground text-xs">
      {{ $t('archive.loadingProjects') }}
    </div>

    <!-- 错误态 -->
    <div v-else-if="error" class="px-3 py-4 text-xs text-destructive">
      {{ error }}
    </div>

    <!-- 空态 -->
    <div v-else-if="archiveProjects.length === 0" class="px-3 py-8 text-center">
      <p class="text-muted-foreground text-xs">{{ $t('archive.noProjects') }}</p>
      <p class="text-muted-foreground text-xs mt-1">{{ $t('archive.checkProjectsDir') }}</p>
    </div>

    <!-- 项目列表 -->
    <div v-else class="flex-1 overflow-y-auto min-h-0 overscroll-y-contain flex flex-col p-1">
      <template
        v-for="(project, i) in archiveProjects"
        :key="project.id"
      >
      <div v-if="i > 0" class="ml-6 mr-2.5 border-t border-border/30" />
      <button
        class="w-full text-left px-2.5 py-2 flex items-start gap-2 rounded-md transition-colors group"
        :class="isSelected(project.id) ? 'bg-card shadow-paper' : 'hover:bg-muted'"
        @click="toggleProject(project.id)"
      >
        <!-- 选中指示 -->
        <span
          class="w-1.5 h-1.5 rounded-full mt-1.5 shrink-0 transition-colors"
          :class="isSelected(project.id) ? 'bg-primary' : 'bg-transparent group-hover:bg-muted-foreground'"
        />
        <div class="min-w-0 flex-1">
          <div class="flex items-baseline justify-between gap-2">
            <div class="min-w-0 flex items-center gap-1.5">
              <span class="text-sm text-foreground truncate font-medium" :title="project.display_path">
                {{ projectName(project.display_path) }}
              </span>
            </div>
            <span v-if="project.last_active" class="text-xs text-muted-foreground shrink-0">
              {{ relativeTime(project.last_active) }}
            </span>
          </div>
          <div v-if="projectPath(project.display_path)" class="text-xs text-muted-foreground/50 truncate mt-px">
            {{ projectPath(project.display_path) }}
          </div>
          <div class="text-xs text-muted-foreground mt-px flex items-baseline justify-between">
            <span>{{ $t('archive.sessionCount', { count: project.session_count }) }}</span>
            <span>{{ formatTokens(projectTokens(project)) }}</span>
          </div>
        </div>
      </button>
      </template>
    </div>

    <!-- 底部统计 -->
    <div class="px-3 py-2 border-t border-border text-xs text-muted-foreground flex gap-3">
      <span>{{ $t('archive.projectStats', { projects: sidebarStats.projectCount }) }}</span>
      <span>{{ $t('archive.sessionStats', { sessions: sidebarStats.sessionCount }) }}</span>
      <span>{{ formatBytes(sidebarStats.totalSize) }}</span>
    </div>
  </div>
</template>

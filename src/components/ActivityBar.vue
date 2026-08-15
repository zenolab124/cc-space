<script setup lang="ts">
import { computed, watchEffect } from 'vue'
import { useI18n } from 'vue-i18n'
import { useUiState, type AppSection } from '@/composables/useUiState'
import { useTheme } from '@/composables/useTheme'
import { useNotifications } from '@/composables/useNotifications'
import { useUpdater } from '@/composables/useUpdater'
import { useEngines } from '@/engines/useEngines'

const { t } = useI18n()
const { activeSection, switchSection } = useUiState()
const { activeTheme, activeThemeLabel, cycleActiveTheme } = useTheme()
const { badgeCount } = useNotifications()
const { status: updateStatus } = useUpdater()
const { engines } = useEngines()
const hasSettingsNotice = computed(() => updateStatus.value === 'available')

const hasWorkshop = computed(() => engines.value.some(engine => engine.enabled && engine.capabilities.facets.assets))
const hasAutomation = computed(() => engines.value.some(engine => engine.enabled && engine.capabilities.facets.automation))

/** 终态七域全摆；v2.1.0 点亮工作台,「会话」更名档案语义,其余灰置 */
interface DomainItem {
  key: string
  icon: string
  label: string
  section?: AppSection
}

const topDomains = computed<DomainItem[]>(() => [
  { key: 'workbench', icon: 'i-carbon-workspace', label: t('activity.workbench'), section: 'workbench' },
  { key: 'sessions', icon: 'i-carbon-chat', label: t('activity.archive'), section: 'sessions' },
  { key: 'search', icon: 'i-carbon-search', label: t('activity.search'), section: 'search' },
  ...(hasWorkshop.value ? [{ key: 'workshop', icon: 'i-carbon-tools', label: t('activity.workshop'), section: 'workshop' as AppSection }] : []),
  ...(hasAutomation.value ? [{ key: 'automation', icon: 'i-carbon-bot', label: t('activity.automation'), section: 'automation' as AppSection }] : []),
])

watchEffect(() => {
  if (engines.value.length === 0) return
  if (activeSection.value === 'workshop' && !hasWorkshop.value) switchSection('workbench')
  if (activeSection.value === 'automation' && !hasAutomation.value) switchSection('workbench')
})

function onItemClick(item: DomainItem) {
  if (item.section) switchSection(item.section)
}

/** 工作台角标(FR-007):未处理持久型事件数(等权限+出错),0 隐藏,上限 9+ */
const badgeText = computed(() => {
  if (badgeCount.value <= 0) return null
  return badgeCount.value > 9 ? '9+' : String(badgeCount.value)
})
</script>

<template>
  <nav
    class="w-12 shrink-0 flex flex-col items-center gap-1 pt-1.5 pb-2.5 bg-secondary border-r border-border"
    data-tauri-drag-region
  >

    <button
      v-for="item in topDomains"
      :key="item.key"
      class="ab-item"
      :class="item.section
        ? { active: activeSection === item.section }
        : 'ab-disabled'"
      :title="item.section ? item.label : $t('activity.comingSoon', { label: item.label })"
      :disabled="!item.section"
      @click="onItemClick(item)"
    >
      <span :class="item.icon" class="w-4.5 h-4.5 block" />
      <span
        v-if="item.key === 'workbench' && badgeText"
        class="ab-badge"
      >{{ badgeText }}</span>
    </button>

    <div class="flex-1" data-tauri-drag-region />

    <!-- 外观切换（自 Toolbar 移入，全域可达） -->
    <button class="ab-item" :title="activeThemeLabel" @click="cycleActiveTheme">
      <span :class="activeTheme.icon" class="w-4.5 h-4.5 block" />
    </button>

    <!-- 设置(自多渠道起点亮:渠道管理是第一个设置域) -->
    <button
      class="ab-item"
      :class="{ active: activeSection === 'settings' }"
      :title="$t('activity.settings')"
      @click="switchSection('settings')"
    >
      <span class="i-carbon-settings w-4.5 h-4.5 block" />
      <span v-if="hasSettingsNotice" class="ab-dot" />
    </button>
  </nav>
</template>

<style scoped>
.ab-item {
  width: 34px;
  height: 34px;
  border-radius: var(--radius);
  display: grid;
  place-items: center;
  color: var(--muted-foreground);
  position: relative;
  transition: color 0.15s, background-color 0.15s;
}
.ab-item:not(.ab-disabled):hover {
  color: var(--foreground);
  background: var(--muted);
}
/* 选中态 =「纸片拈起」：card 底 + paper 阴影 + 左侧 2px primary 指示条 */
.ab-item.active {
  color: var(--primary);
  background: var(--card);
  box-shadow: var(--shadow-paper);
}
.ab-item.active::before {
  content: '';
  position: absolute;
  left: -7px;
  top: 8px;
  bottom: 8px;
  width: 2px;
  border-radius: 1px;
  background: var(--primary);
}
/* 灰置域：无 hover 反馈 */
.ab-disabled {
  opacity: 0.35;
  cursor: default;
}
/* 工作台角标:accent 单一视觉信号,不随事件类型变色 */
.ab-badge {
  position: absolute;
  top: 2px;
  right: 2px;
  min-width: 13px;
  height: 13px;
  border-radius: 3px;
  background: var(--accent);
  color: var(--accent-foreground);
  font-size: 9px;
  line-height: 13px;
  text-align: center;
  padding: 0 2px;
}
.ab-dot {
  position: absolute;
  top: 4px;
  right: 4px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--primary);
}
</style>

<script setup lang="ts">
import { computed } from 'vue'
import { useUiState } from '@/composables/useUiState'
import type { WorkbenchTab } from '@/composables/useWorkbench'
import MonitorRail from './MonitorRail.vue'
import WorkbenchColumns from './WorkbenchColumns.vue'
import RaceColumns from './RaceColumns.vue'

const props = defineProps<{
  tab: WorkbenchTab
}>()

const { monitorRailCollapsed, monitorRailPeeking, monitorPeekInstantHide, peekMonitorRail, unpeekMonitorRail } = useUiState()
const isRaceTab = computed(() => !!props.tab.race)
</script>

<template>
  <div class="h-full flex min-h-0 relative">
    <template v-if="!isRaceTab">
      <!-- 常驻模式 -->
      <MonitorRail v-show="!monitorRailCollapsed" :tab="tab" />

      <!-- 抽屉模式：收起时 hover 浮出 -->
      <Transition :name="monitorPeekInstantHide ? '' : 'rail-peek'">
        <aside
          v-if="monitorRailCollapsed && monitorRailPeeking"
          class="rail-drawer"
          @mouseenter="peekMonitorRail"
          @mouseleave="unpeekMonitorRail"
        >
          <MonitorRail :tab="tab" />
        </aside>
      </Transition>

      <WorkbenchColumns :tab="tab" />
    </template>

    <!-- 赛马模式:全宽列 + 共享输入 -->
    <RaceColumns v-else :tab="tab" />
  </div>
</template>

<style scoped>
.rail-drawer {
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  z-index: 30;
  background: var(--background);
  border-right: 1px solid var(--border);
  box-shadow: 6px 0 16px rgb(0 0 0 / 0.12), 2px 0 4px rgb(0 0 0 / 0.06);
}

.rail-peek-enter-active,
.rail-peek-leave-active {
  transition: transform 150ms ease, opacity 150ms ease;
}
.rail-peek-enter-from,
.rail-peek-leave-to {
  transform: translateX(-100%);
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .rail-peek-enter-active,
  .rail-peek-leave-active {
    transition: none !important;
  }
}
</style>

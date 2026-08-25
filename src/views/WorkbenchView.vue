<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import WorkbenchTabPane from '@/components/workbench/WorkbenchTabPane.vue'
import { useWorkbench } from '@/composables/useWorkbench'
import { touchRecentTab } from '@/utils/recentTabCache'

const RECENT_TAB_CACHE_LIMIT = 4
const { state, activeTab } = useWorkbench()
const cachedTabIds = ref<string[]>([])

function refreshTabCache(activeId: string) {
  const validIds = new Set(state.value.tabs.map(tab => tab.id))
  cachedTabIds.value = touchRecentTab(
    cachedTabIds.value,
    activeId,
    validIds,
    RECENT_TAB_CACHE_LIMIT,
  )
}

watch(
  [() => activeTab.value.id, () => state.value.tabs.map(tab => tab.id).join('\0')],
  ([activeId]) => refreshTabCache(activeId),
  { immediate: true, flush: 'sync' },
)

const cachedTabs = computed(() => cachedTabIds.value.flatMap(id => {
  const tab = state.value.tabs.find(candidate => candidate.id === id)
  return tab ? [tab] : []
}))
</script>

<template>
  <div class="h-full min-h-0 relative">
    <WorkbenchTabPane
      v-for="tab in cachedTabs"
      v-show="tab.id === activeTab.id"
      :key="tab.id"
      :tab="tab"
    />
  </div>
</template>

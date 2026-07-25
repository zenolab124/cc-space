<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useMemory } from '@/composables/useMemory'
import MemoryListColumn from './memory/MemoryListColumn.vue'
import MemoryHealthPanel from './memory/MemoryHealthPanel.vue'
import MemoryDetailPane from './memory/MemoryDetailPane.vue'
import MemoryEditor from './memory/MemoryEditor.vue'
import MemoryDeleteDialog from './memory/MemoryDeleteDialog.vue'

/**
 * 记忆子页（v2.9.0 FR-007/FR-008/FR-009）：
 * 自含「列表列+详情区」两列布局。体检面板展开/收起。
 * 编辑态与删除确认对话框覆盖在详情区内。
 */

const { t } = useI18n()
const { loading, error, overview, refresh, ensureFresh } = useMemory()

// 每次挂载（= 每次切进记忆子页）重扫，外部改动即时可见。
// 仍不在 useMemory 里自动加载——避免六视图常驻 mount 把 IO 提前到启动期
onMounted(() => {
  ensureFresh()
})

// 体检面板展开/收起
const healthExpanded = ref(false)
function toggleHealth() {
  healthExpanded.value = !healthExpanded.value
}

// 编辑态
const editing = ref(false)
function startEdit() {
  editing.value = true
  deleting.value = false
}
function closeEdit() {
  editing.value = false
}

// 删除确认
const deleting = ref(false)
function startDelete() {
  deleting.value = true
  editing.value = false
}
function closeDelete() {
  deleting.value = false
}
</script>

<template>
  <!-- 全局加载/错误/空态 -->
  <div v-if="loading && !overview" class="memory-empty">
    <span class="text-xs text-muted-foreground">{{ t('common.loading') }}</span>
  </div>
  <div v-else-if="error" class="memory-empty">
    <div class="text-xs text-destructive">{{ error }}</div>
    <button class="ws-btn mt-2" @click="refresh">{{ t('common.retry') }}</button>
  </div>
  <div v-else-if="overview && overview.projects.length === 0" class="memory-empty">
    <span class="text-xs text-muted-foreground">{{ t('memory.noProjects') }}</span>
  </div>

  <!-- 正常：两列布局 -->
  <div v-else class="memory-layout">
    <!-- 列表列（含体检面板） -->
    <MemoryListColumn @toggle-health="toggleHealth">
      <template #health>
        <MemoryHealthPanel v-if="healthExpanded" />
      </template>
    </MemoryListColumn>

    <!-- 详情区 -->
    <div class="memory-detail-wrapper">
      <!-- 编辑态 -->
      <template v-if="editing">
        <MemoryEditor @close="closeEdit" />
      </template>
      <!-- 删除确认 -->
      <template v-else-if="deleting">
        <MemoryDetailPane @edit="startEdit" @delete="startDelete" />
        <MemoryDeleteDialog @close="closeDelete" />
      </template>
      <!-- 正常详情 -->
      <template v-else>
        <MemoryDetailPane @edit="startEdit" @delete="startDelete" />
      </template>
    </div>
  </div>
</template>

<style scoped>
.memory-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  min-height: 200px;
  gap: 8px;
}
.memory-layout {
  display: flex;
  height: 100%;
}
.memory-detail-wrapper {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}
.ws-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 3px 10px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--card);
  cursor: pointer;
  color: var(--foreground);
}
.ws-btn:hover {
  background: var(--muted);
}
.mt-2 {
  margin-top: 8px;
}
</style>

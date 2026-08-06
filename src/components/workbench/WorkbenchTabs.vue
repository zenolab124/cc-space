<script setup lang="ts">
import { nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Menu } from '@tauri-apps/api/menu'
import { useWorkbench, type WorkbenchTab } from '@/composables/useWorkbench'
import { useConfirm } from '@/composables/useConfirm'

/**
 * 工作台 tab 条(FR-001):创建/重命名/关闭;溢出时横向滚动,不换行、不下拉收纳。
 * tab 不参与拖拽——排序与跨台移动会话均已废弃(切走再拖入的路径无价值)。
 */
const { t } = useI18n()
const { state, activeTab, setActiveTab, createTab, renameTab, closeTab } = useWorkbench()
const { confirm } = useConfirm()

// --- 重命名(双击 / 右键菜单触发;Esc 取消、失焦或 Enter 确认) ---

const editingTabId = ref<string | null>(null)
const editingName = ref('')

function startRename(tab: WorkbenchTab) {
  editingTabId.value = tab.id
  editingName.value = tab.name
  void nextTick(() => {
    if (editingTabId.value !== tab.id) return
    editInputElement.value?.focus()
    editInputElement.value?.select()
  })
}

/** v-for 内的 template ref 会被收集为数组,函数 ref 只负责捕获当前输入框 */
const editInputElement = ref<HTMLInputElement | null>(null)
function captureEditInput(el: unknown) {
  editInputElement.value = el instanceof HTMLInputElement ? el : null
}

function commitRename() {
  if (editingTabId.value) {
    renameTab(editingTabId.value, editingName.value)
  }
  editingTabId.value = null
}

function cancelRename() {
  editingTabId.value = null
}

// --- 关闭(含会话需确认;最后一个不可关) ---

async function requestClose(tab: WorkbenchTab) {
  if (state.value.tabs.length <= 1) return
  if (tab.sessionIds.length > 0) {
    const ok = await confirm(t('workbench.closeConfirm', { count: tab.sessionIds.length }), t('common.close'))
    if (!ok) return
  }
  closeTab(tab.id)
}

// --- 右键菜单 ---

async function onContextMenu(e: MouseEvent, tab: WorkbenchTab) {
  e.preventDefault()
  const menu = await Menu.new({
    items: [
      {
        id: 'rename',
        text: t('workbench.rename'),
        action: () => startRename(tab),
      },
      {
        id: 'close',
        text: t('common.close'),
        enabled: state.value.tabs.length > 1,
        action: () => void requestClose(tab),
      },
    ],
  })
  await menu.popup()
}
</script>

<template>
  <div
    class="h-full flex items-center gap-0.5 pr-2 overflow-x-auto tabs-scroll"
  >
    <div
      v-for="tab in state.tabs"
      :key="tab.id"
      class="wb-tab"
      :class="{ active: tab.id === activeTab.id }"
      @click="setActiveTab(tab.id)"
      @dblclick="startRename(tab)"
      @contextmenu="onContextMenu($event, tab)"
    >
      <input
        v-if="editingTabId === tab.id"
        :ref="captureEditInput"
        v-model="editingName"
        class="w-24 bg-transparent border-none outline-none text-xs text-foreground"
        maxlength="20"
        @keydown.enter.prevent="commitRename"
        @keydown.esc.prevent="cancelRename"
        @blur="commitRename"
        @click.stop
        @pointerdown.stop
      />
      <template v-else>
        <span v-if="tab.race" class="i-app-horse w-3 h-3 shrink-0 text-muted-foreground" />
        <span class="truncate max-w-36">{{ tab.name }}</span>
        <span v-if="tab.sessionIds.length > 0" class="text-[10px] text-muted-foreground">{{ tab.sessionIds.length }}</span>
        <button
          v-if="state.tabs.length > 1"
          class="wb-tab-close i-carbon-close"
          :title="$t('common.close')"
          @click.stop="requestClose(tab)"
          @pointerdown.stop
        />
      </template>
    </div>

    <button
      class="wb-tab add shrink-0"
      :title="$t('workbench.newTab')"
      @click="createTab()"
    >＋</button>

    <div class="flex-1 min-w-4 self-stretch" data-tauri-drag-region />
  </div>
</template>

<style scoped>
.wb-tab {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  padding: 2px 10px;
  border-radius: var(--radius);
  color: var(--muted-foreground);
  position: relative;
  white-space: nowrap;
  flex-shrink: 0;
  height: 22px;
  cursor: default;
}
.wb-tab:hover {
  background: var(--muted);
}
.wb-tab.active {
  background: var(--card);
  box-shadow: var(--shadow-paper);
  color: var(--foreground);
  font-weight: 500;
}
.wb-tab.add {
  padding: 4px 8px;
  cursor: pointer;
}
/* 关闭按钮:hover 常显、active tab 常显;最后一个 tab 不渲染此按钮(见 v-if)。
   width/height 归零替代 v-if 隐藏,避免 tab 尺寸随 hover 抖动 */
.wb-tab-close {
  width: 0;
  height: 12px;
  color: var(--muted-foreground);
  opacity: 0;
  transition: width 0.12s ease, opacity 0.12s ease;
  cursor: pointer;
  border-radius: 3px;
}
.wb-tab-close:hover {
  background: var(--accent);
  color: var(--foreground);
}
.wb-tab:hover .wb-tab-close,
.wb-tab.active .wb-tab-close {
  width: 12px;
  opacity: 0.7;
}
/* tab 条横向滚动:细滚动条 */
.tabs-scroll {
  scrollbar-width: thin;
}
</style>

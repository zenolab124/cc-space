<script setup lang="ts">
import { nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Menu } from '@tauri-apps/api/menu'
import { useWorkbench, type WorkbenchTab } from '@/composables/useWorkbench'
import { useConfirm } from '@/composables/useConfirm'
import SortableWorkbenchTab from './SortableWorkbenchTab.vue'

/**
 * 工作台 tab 条(FR-001):创建/重命名/关闭/排序;溢出时横向滚动,不换行、不下拉收纳。
 * Tab 排序只改变展示顺序，不承载会话跨台移动语义。
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
    role="tablist"
  >
    <SortableWorkbenchTab
      v-for="(tab, index) in state.tabs"
      :key="tab.id"
      :tab-id="tab.id"
      :index="index"
      :active="tab.id === activeTab.id"
      :disabled="state.tabs.length <= 1 || editingTabId === tab.id"
      @activate="setActiveTab(tab.id)"
      @rename="startRename(tab)"
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
    </SortableWorkbenchTab>

    <button
      class="wb-tab-add shrink-0"
      :title="$t('workbench.newTab')"
      @click="createTab()"
    >＋</button>

    <div class="flex-1 min-w-4 self-stretch" data-tauri-drag-region />
  </div>
</template>

<style scoped>
.wb-tab-add {
  display: inline-flex;
  height: 22px;
  padding: 4px 8px;
  align-items: center;
  border-radius: var(--radius);
  color: var(--muted-foreground);
  font-size: 11px;
  cursor: pointer;
}
.wb-tab-add:hover { background: var(--muted); }
.wb-tab-add:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 1px;
}
/* tab 条横向滚动:细滚动条 */
.tabs-scroll {
  scrollbar-width: thin;
}
</style>

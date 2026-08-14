<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useWorkbench, setRightZoneWidth } from '@/composables/useWorkbench'
import { useHorizontalWheelScroll } from '@/composables/useHorizontalWheelScroll'
import { useColumnResize } from '@/composables/useColumnResize'
import { useRaceInput } from '@/composables/useRaceInput'
import { useProjects } from '@/composables/useProjects'
import { useConfirm } from '@/composables/useConfirm'
import { shortModel, formatTokens, type TokenUsage, type SessionSummary } from '@/types'
import WorkbenchColumnView from './WorkbenchColumn.vue'
import { shouldSubmitComposer } from '@/components/session/composerAction'

const { activeTab, minColumnWidth, suppressColumnTransition } = useWorkbench()
const { dragging, shiftDragging, onDividerMouseDown } = useColumnResize()
const { t } = useI18n()
const { projects } = useProjects()
const { confirm } = useConfirm()

async function onResetRace() {
  if (broadcasting.value || raceMutationLoading.value) return
  const ok = await confirm(t('workbench.race.resetConfirm'), t('workbench.race.reset'))
  if (!ok) return
  await resetAllLanes()
}

const race = computed(() => activeTab.value.race!)

const {
  inputText,
  textareaRef,
  dropAreaRef,
  imageInput,
  raceError,
  raceMutationLoading,
  broadcasting,
  canSwitchRaceEngine,
  anyStreaming,
  streamingCount,
  broadcastSend,
  stopAll,
  forkNewLane,
  resetAllLanes,
  switchLaneEngine,
} = useRaceInput(activeTab)

const containerRef = ref<HTMLElement>()
const showHud = ref(false)
const enginePickerSessionId = ref<string | null>(null)
const selectingRaceEngine = ref<{ sessionId: string; engineId: string } | null>(null)

function toggleRaceEnginePicker(sessionId: string) {
  if (!canSwitchRaceEngine.value || broadcasting.value || raceMutationLoading.value) return
  enginePickerSessionId.value = enginePickerSessionId.value === sessionId ? null : sessionId
}

function selectingEngineForLane(sessionId: string): string | null {
  return selectingRaceEngine.value?.sessionId === sessionId
    ? selectingRaceEngine.value.engineId
    : null
}

async function selectRaceEngine(sessionId: string, engineId: string) {
  if (selectingRaceEngine.value || broadcasting.value || raceMutationLoading.value) return
  selectingRaceEngine.value = { sessionId, engineId }
  try {
    if (await switchLaneEngine(sessionId, engineId)) enginePickerSessionId.value = null
  } finally {
    selectingRaceEngine.value = null
  }
}

watch(canSwitchRaceEngine, (available) => {
  if (!available) enginePickerSessionId.value = null
})

function getSessionSummary(sessionId: string): SessionSummary | null {
  for (const p of projects.value) {
    const s = p.sessions.find(s => s.id === sessionId)
    if (s) return s
  }
  return null
}

function cacheHitRate(t: TokenUsage): string {
  const total = t.input_tokens + t.cache_read_input_tokens + t.cache_creation_input_tokens
  return total > 0 ? Math.round(t.cache_read_input_tokens / total * 100) + '%' : '—'
}

function cacheOverallRate(t: TokenUsage): string {
  const total = t.input_tokens + t.output_tokens + t.cache_read_input_tokens + t.cache_creation_input_tokens
  return total > 0 ? Math.round(t.cache_read_input_tokens / total * 100) + '%' : '—'
}

let resizeObserver: ResizeObserver | null = null

useHorizontalWheelScroll(containerRef)

onMounted(() => {
  const el = containerRef.value
  if (!el) return
  setRightZoneWidth(el.clientWidth)
  resizeObserver = new ResizeObserver((entries) => {
    for (const entry of entries) {
      if (entry.contentRect.width > 0) setRightZoneWidth(entry.contentRect.width)
    }
  })
  resizeObserver.observe(el)
  imageInput.attach()
})

onUnmounted(() => {
  resizeObserver?.disconnect()
  resizeObserver = null
})

function autoResize() {
  const el = textareaRef.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 160) + 'px'
}

function onInputKeydown(e: KeyboardEvent) {
  if (shouldSubmitComposer(e)) {
    e.preventDefault()
    broadcastSend()
  }
}
</script>

<template>
  <div ref="dropAreaRef" class="flex-1 min-w-0 h-full flex flex-col">
    <div class="flex-1 min-h-0 flex flex-row">
      <!-- 赛道区(横向滚动) -->
      <div ref="containerRef" data-workbench-panorama class="flex-1 min-w-0 overflow-x-auto flex flex-row p-2.5 gap-2.5">
        <div
          v-for="(col, i) in activeTab.columns"
          :key="col.id"
          class="h-full relative shrink-0 race-col"
          data-workbench-column
          :class="{ 'no-transition': dragging || suppressColumnTransition }"
          :style="{
            width: `${activeTab.columnSizes[i] ?? minColumnWidth}px`,
            flex: `${activeTab.columnSizes[i] ?? minColumnWidth} 0 auto`,
          }"
        >
          <WorkbenchColumnView
            :column="col"
            :tab-id="activeTab.id"
            :index="i"
            :mutation-disabled="broadcasting || raceMutationLoading"
            :engine-switch-available="canSwitchRaceEngine"
            :engine-picker-open="enginePickerSessionId === col.sessionId"
            :selecting-engine-id="selectingEngineForLane(col.sessionId)"
            @toggle-race-engine-picker="toggleRaceEnginePicker"
            @select-race-engine="selectRaceEngine"
          />

          <!-- 列右边缘 resize 手柄(与普通多列同款,Shift 调全局最小列宽) -->
          <div
            class="absolute top-0 bottom-0 -right-[7px] w-[14px] cursor-col-resize z-30"
            :class="{ 'divider-shift': shiftDragging }"
            @mousedown="onDividerMouseDown($event, i)"
          />

          <!-- Token HUD 覆盖层 -->
          <div
            v-if="showHud"
            class="race-hud"
          >
            <template v-if="getSessionSummary(col.sessionId)">
              <div class="hud-row font-medium">
                <span>{{ shortModel(getSessionSummary(col.sessionId)!.model ?? '') }}</span>
              </div>
              <div class="hud-divider" />
              <div class="hud-row">
                <span>input_tokens</span>
                <span>{{ formatTokens(getSessionSummary(col.sessionId)!.total_tokens.input_tokens) }}</span>
              </div>
              <div class="hud-row">
                <span>output_tokens</span>
                <span>{{ formatTokens(getSessionSummary(col.sessionId)!.total_tokens.output_tokens) }}</span>
              </div>
              <div class="hud-row">
                <span>cache_creation</span>
                <span>{{ formatTokens(getSessionSummary(col.sessionId)!.total_tokens.cache_creation_input_tokens) }}</span>
              </div>
              <div class="hud-row">
                <span>cache_read</span>
                <span>{{ formatTokens(getSessionSummary(col.sessionId)!.total_tokens.cache_read_input_tokens) }}</span>
              </div>
              <div class="hud-divider" />
              <div class="hud-row">
                <span>{{ $t('topbar.tokenTotalInput') }}</span>
                <span>{{ formatTokens(getSessionSummary(col.sessionId)!.total_tokens.input_tokens + getSessionSummary(col.sessionId)!.total_tokens.cache_creation_input_tokens + getSessionSummary(col.sessionId)!.total_tokens.cache_read_input_tokens) }}</span>
              </div>
              <div class="hud-row">
                <span>{{ $t('topbar.tokenTotalOutput') }}</span>
                <span>{{ formatTokens(getSessionSummary(col.sessionId)!.total_tokens.output_tokens) }}</span>
              </div>
              <div class="hud-row">
                <span>{{ $t('topbar.tokenCacheHitRate') }}</span>
                <span>{{ cacheHitRate(getSessionSummary(col.sessionId)!.total_tokens) }}</span>
              </div>
              <div class="hud-row">
                <span>{{ $t('topbar.tokenCacheRatio') }}</span>
                <span>{{ cacheOverallRate(getSessionSummary(col.sessionId)!.total_tokens) }}</span>
              </div>
              <div class="hud-divider" />
              <div class="hud-row font-medium">
                <span>{{ $t('topbar.tokenTotal') }}</span>
                <span>{{ formatTokens(
                  getSessionSummary(col.sessionId)!.total_tokens.input_tokens +
                  getSessionSummary(col.sessionId)!.total_tokens.output_tokens +
                  getSessionSummary(col.sessionId)!.total_tokens.cache_read_input_tokens +
                  getSessionSummary(col.sessionId)!.total_tokens.cache_creation_input_tokens
                ) }}</span>
              </div>
            </template>
          </div>
        </div>
      </div>

      <!-- 右侧工具栏 -->
      <div class="shrink-0 w-10 flex flex-col items-center gap-2 py-2.5 border-l border-border bg-background">
        <button
          :disabled="raceMutationLoading || broadcasting"
          class="icon-btn icon-btn-lg"
          :class="showHud && 'icon-btn-active'"
          v-tooltip="$t('workbench.race.tokenHud')"
          @click="showHud = !showHud"
        >
          <span class="i-carbon-dashboard w-3.5 h-3.5" />
        </button>
        <button
          :disabled="raceMutationLoading || broadcasting"
          class="icon-btn icon-btn-lg"
          v-tooltip="$t('workbench.race.reset')"
          @click="onResetRace"
        >
          <span class="i-carbon-reset w-3.5 h-3.5" />
        </button>
        <button
          :disabled="raceMutationLoading || broadcasting"
          class="icon-btn icon-btn-lg icon-btn-dashed flex-1"
          v-tooltip="t('workbench.race.addLane')"
          @click="forkNewLane"
        >
          <span class="i-carbon-add w-3.5 h-3.5" />
        </button>
      </div>
    </div>

    <!-- 输入区 -->
    <div
      class="px-4 py-3 border-t border-border shrink-0 transition-colors"
      :class="imageInput.isDragging.value && 'ring-1 ring-primary/40 ring-inset bg-primary/5'"
    >
      <div v-if="raceError" class="mb-1 text-xs text-destructive">
        {{ raceError }}
      </div>

      <!-- 拖拽指引(pointer-events-none:避免提示自身触发 dragleave 抖动) -->
      <div
        v-if="imageInput.isDragging.value"
        class="mb-1 text-xs text-primary flex items-center gap-1.5 pointer-events-none"
      >
        <span class="i-carbon-image w-3.5 h-3.5" />
        {{ t('image.dropHint') }}
      </div>

      <div v-if="imageInput.images.value.length" class="mb-2 flex gap-2 flex-wrap">
        <div v-for="img in imageInput.images.value" :key="img.id" class="relative w-14 h-14 rounded border border-border overflow-hidden group">
          <img :src="img.dataUrl" class="w-full h-full object-cover" />
          <button
            class="absolute top-0 right-0 w-4 h-4 rounded-bl bg-destructive/80 text-destructive-foreground flex items-center justify-center text-2.5 leading-none opacity-0 group-hover:opacity-100 transition-opacity"
            @click="imageInput.removeImage(img.id)"
          >&times;</button>
        </div>
      </div>

      <div v-if="imageInput.lastError.value" class="mb-1 text-xs text-destructive">
        {{ imageInput.lastError.value.message }}
      </div>

      <div class="flex items-center gap-2">
        <textarea
          ref="textareaRef"
          v-model="inputText"
          :placeholder="t('workbench.race.sharedInput')"
          rows="1"
          :disabled="broadcasting || raceMutationLoading"
          class="flex-1 px-3 py-2 text-sm rounded-md bg-popover border border-border text-foreground placeholder-muted-foreground resize-none overflow-x-hidden placeholder:[white-space:pre-wrap] focus:outline-none focus:border-ring transition-colors disabled:cursor-not-allowed disabled:opacity-50"
          @keydown="onInputKeydown"
          @input="autoResize"
        />

        <button
          v-if="anyStreaming && !inputText.trim() && !imageInput.images.value.length"
          :disabled="broadcasting || raceMutationLoading"
          class="px-3 py-2 text-xs rounded-md bg-accent text-accent-foreground hover:shadow-paper transition-shadow shrink-0"
          @click="stopAll"
        >
          {{ t('workbench.race.stopAll') }}
          <span v-if="streamingCount > 0" class="ml-1 opacity-60">{{ streamingCount }}/{{ race.lanes.length }}</span>
        </button>

        <button
          v-else
          :disabled="broadcasting || raceMutationLoading || (!inputText.trim() && !imageInput.images.value.length)"
          class="px-3 py-2 text-xs rounded-md bg-primary text-primary-foreground hover:shadow-paper transition-shadow shrink-0 disabled:opacity-30 disabled:cursor-not-allowed"
          @click="broadcastSend"
        >
          {{ t('common.send') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 与 SortableColumn 同款宽度过渡:关道归零动画依赖它 */
.race-col {
  min-width: 0;
  transition: width 250ms cubic-bezier(0.32, 0.72, 0, 1);
  overflow: hidden;
}
.no-transition {
  transition: none !important;
}
.divider-shift {
  background: color-mix(in srgb, var(--primary) 25%, transparent);
}

.race-hud {
  position: absolute;
  top: 40px;
  right: 6px;
  z-index: 20;
  padding: 6px 10px;
  border-radius: 6px;
  background: var(--foreground);
  color: var(--background);
  opacity: 0.75;
  font-size: 11px;
  line-height: 1.6;
  font-variant-numeric: tabular-nums;
  pointer-events: none;
  min-width: 130px;
}
.hud-row {
  display: flex;
  justify-content: space-between;
  gap: 12px;
}
.hud-sep {
  opacity: 0.5;
}
.hud-divider {
  border-top: 1px solid currentColor;
  opacity: 0.2;
  margin: 2px 0;
}
</style>

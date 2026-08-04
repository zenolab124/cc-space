<script setup lang="ts">
/**
 * ExitPlanMode 计划批准卡
 *
 * Claude 在计划模式完成方案后经权限通道请求批准：
 *   - 批准计划 → allow（CLI 切回普通模式开始动手，后续写操作仍逐项询问）
 *   - 打回修改 → deny + message（修改意见随 deny 回传，Claude 继续完善计划）
 *
 * 键盘：Enter(焦点不在输入框) → 批准；Esc → 收起意见输入。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { renderMarkdownCached } from '@/composables/useMarkdown'
import type { PermissionRequest, PermissionDecision, RespondExtra } from '@/composables/usePermissionRequests'
import SessionInteractionCard from '@/components/session/SessionInteractionCard.vue'

const props = defineProps<{
  request: PermissionRequest
}>()

const { t } = useI18n()

const emit = defineEmits<{
  (e: 'decide', decision: PermissionDecision, extra?: RespondExtra): void
}>()

const plan = computed(() => {
  const v = props.request.input.plan
  return typeof v === 'string' ? v : ''
})

const renderedPlan = computed(() => renderMarkdownCached(plan.value))

/** 打回意见输入态 */
const rejecting = ref(false)
const feedback = ref('')
const feedbackBox = ref<HTMLTextAreaElement | null>(null)

function approve() {
  emit('decide', 'allow_once')
}

function openReject() {
  rejecting.value = true
  void nextTick(() => feedbackBox.value?.focus())
}

function sendReject() {
  const msg = feedback.value.trim()
  emit('decide', 'deny', {
    message: msg ? t('permission.plan.rejectWithFeedback', { msg }) : t('permission.plan.rejectDefault'),
  })
}

function inTextInput(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && ['INPUT', 'TEXTAREA'].includes(target.tagName)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !inTextInput(e.target)) {
    e.preventDefault()
    e.stopPropagation()
    if (!rejecting.value) approve()
    return
  }
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    if (inTextInput(e.target)) {
      ;(e.target as HTMLElement).blur()
      return
    }
    if (rejecting.value) rejecting.value = false
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown, { capture: true }))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown, { capture: true } as any))
</script>

<template>
  <SessionInteractionCard
    role="dialog"
    :aria-label="$t('permission.plan.title')"
  >
    <!-- 头部 -->
    <div class="flex items-center gap-2 px-3 py-2 border-b border-border">
      <span class="i-carbon-task-approved w-4 h-4 text-primary shrink-0" aria-hidden="true" />
      <span class="text-sm font-medium text-foreground">{{ $t('permission.plan.title') }}</span>
      <span class="text-xs text-muted-foreground">{{ $t('permission.plan.subtitle') }}</span>
    </div>

    <!-- 计划正文 -->
    <div class="px-3 py-2 max-h-96 overflow-y-auto">
      <div v-if="plan" class="prose-msg text-xs" v-html="renderedPlan" />
      <div v-else class="text-xs text-muted-foreground">{{ $t('permission.plan.empty') }}</div>
    </div>

    <!-- 打回意见输入 -->
    <div v-if="rejecting" class="px-3 pb-2">
      <textarea
        ref="feedbackBox"
        v-model="feedback"
        class="feedback-box"
        rows="3"
        :placeholder="$t('permission.plan.rejectPlaceholder')"
      />
    </div>

    <!-- 底部按钮 -->
    <div class="flex items-center gap-2 px-3 py-2 border-t border-border">
      <template v-if="!rejecting">
        <button type="button" class="btn btn-primary" @click="approve">
          <span class="i-carbon-checkmark w-3.5 h-3.5" aria-hidden="true" />
          {{ $t('permission.plan.approve') }}
        </button>
        <div class="flex-1" />
        <button type="button" class="btn btn-ghost" @click="openReject">
          <span class="i-carbon-edit w-3.5 h-3.5" aria-hidden="true" />
          {{ $t('permission.plan.reject') }}
        </button>
      </template>
      <template v-else>
        <button type="button" class="btn btn-primary" @click="sendReject">
          <span class="i-carbon-send w-3.5 h-3.5" aria-hidden="true" />
          {{ $t('permission.plan.sendFeedback') }}
        </button>
        <div class="flex-1" />
        <button type="button" class="btn btn-ghost" @click="rejecting = false">
          {{ $t('common.back') }}
        </button>
      </template>
    </div>
  </SessionInteractionCard>
</template>

<style scoped>
.plan-card {
  min-width: 320px;
  max-width: 640px;
  width: 100%;
}

.feedback-box {
  width: 100%;
  padding: 6px 8px;
  border: 1px solid var(--border);
  border-radius: 4px;
  background-color: var(--background);
  color: var(--foreground);
  font-size: 12px;
  line-height: 1.5;
  resize: vertical;
  outline: none;
}

.feedback-box:focus {
  border-color: var(--primary);
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border-radius: 4px;
  font-size: 12px;
  line-height: 1.4;
  font-weight: 500;
  transition: background-color 120ms ease, color 120ms ease, box-shadow 120ms ease;
  cursor: pointer;
  border: 1px solid transparent;
  outline: none;
}

.btn:focus-visible {
  box-shadow: 0 0 0 2px var(--ring);
}

.btn-primary {
  background-color: var(--primary);
  color: var(--primary-foreground);
}
.btn-primary:hover {
  box-shadow: var(--shadow-paper);
}

.btn-ghost {
  border-color: var(--border);
  color: var(--muted-foreground);
}
.btn-ghost:hover {
  background-color: var(--muted);
}

@media (prefers-reduced-motion: reduce) {
  .btn {
    transition: none !important;
  }
}
</style>

<script setup lang="ts">
/**
 * AskUserQuestion 可交互问答卡
 *
 * 经权限通道下发，但语义是「Claude 向用户提问」——不是授权。
 * 用户作答后以 allow + updatedInput { ...原input, answers } 回传：
 *   - answers 的 key = 问题文本(question 字段)
 *   - 单选 value = 选中项 label(string)；多选 value = label 数组
 *   - 「其他」自定义输入 = 用户自由文本直接作 value
 * 「跳过不答」= deny + message，Claude 会收到反馈自行调整。
 *
 * 键盘：Enter(焦点不在输入框且已全部作答) → 提交；Esc → 输入框失焦/跳过。
 */
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { parseQuestions } from '@/utils/askQuestions'
import type { PermissionRequest, PermissionDecision, RespondExtra } from '@/composables/usePermissionRequests'
import SessionInteractionCard from '@/components/session/SessionInteractionCard.vue'

const props = defineProps<{
  request: PermissionRequest
}>()

const { t } = useI18n()

const emit = defineEmits<{
  (e: 'decide', decision: PermissionDecision, extra?: RespondExtra): void
}>()

const questions = computed(() => parseQuestions(props.request.input))

/** 「其他」选项的虚拟索引 */
const OTHER = -1

/** 每题选中态：单选存索引(null=未答)，多选存索引集合 */
const picked = reactive<Record<number, number | null>>({})
const pickedMulti = reactive<Record<number, Set<number>>>({})
/** 每题「其他」自由文本 */
const otherTexts = reactive<Record<number, string>>({})

const otherInputs = ref<Record<number, HTMLInputElement | null>>({})

function setOtherInput(qi: number, el: unknown) {
  otherInputs.value[qi] = (el as HTMLInputElement | null) ?? null
}

function isPicked(qi: number, oi: number): boolean {
  const q = questions.value[qi]
  return q?.multiSelect ? (pickedMulti[qi]?.has(oi) ?? false) : picked[qi] === oi
}

function toggle(qi: number, oi: number) {
  const q = questions.value[qi]
  if (!q) return
  if (q.multiSelect) {
    const set = pickedMulti[qi] ?? (pickedMulti[qi] = new Set())
    if (set.has(oi)) set.delete(oi)
    else set.add(oi)
  } else {
    picked[qi] = picked[qi] === oi ? null : oi
  }
  // 选中「其他」时聚焦其输入框
  if (oi === OTHER && isPicked(qi, OTHER)) {
    requestAnimationFrame(() => otherInputs.value[qi]?.focus())
  }
}

/** 单题是否已作答（其他项须有非空文本） */
function answered(qi: number): boolean {
  const q = questions.value[qi]
  if (!q) return false
  const hasOtherText = isPicked(qi, OTHER) && (otherTexts[qi] ?? '').trim().length > 0
  if (q.multiSelect) {
    const set = pickedMulti[qi]
    const realPicks = set ? [...set].filter(i => i !== OTHER).length : 0
    // 勾了「其他」但没填文本 → 视为未答完
    if (set?.has(OTHER) && !hasOtherText) return false
    return realPicks > 0 || hasOtherText
  }
  if (picked[qi] === OTHER) return hasOtherText
  return picked[qi] != null
}

const allAnswered = computed(
  () => questions.value.length > 0 && questions.value.every((_, qi) => answered(qi)),
)

/** 构造 answers：key=问题文本，单选 string / 多选 string[]，其他=自由文本 */
function buildAnswers(): Record<string, string | string[]> {
  const out: Record<string, string | string[]> = {}
  questions.value.forEach((q, qi) => {
    const otherText = (otherTexts[qi] ?? '').trim()
    if (q.multiSelect) {
      const set = pickedMulti[qi] ?? new Set<number>()
      const labels = [...set]
        .filter(i => i !== OTHER)
        .sort((a, b) => a - b)
        .map(i => q.options[i]?.label ?? '')
        .filter(Boolean)
      if (set.has(OTHER) && otherText) labels.push(otherText)
      out[q.question] = labels
    } else {
      out[q.question] = picked[qi] === OTHER ? otherText : (q.options[picked[qi] ?? -2]?.label ?? '')
    }
  })
  return out
}

function submit() {
  if (!allAnswered.value) return
  emit('decide', 'allow_once', {
    updatedInput: { ...props.request.input, answers: buildAnswers() },
  })
}

function skip() {
  emit('decide', 'deny', { message: t('permission.question.skipMessage') })
}

// --- 全局键盘 ---

function inTextInput(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && ['INPUT', 'TEXTAREA'].includes(target.tagName)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    if (inTextInput(e.target)) return
    if (allAnswered.value) {
      e.preventDefault()
      e.stopPropagation()
      submit()
    }
    return
  }
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    // 焦点在输入框内：先失焦，不直接跳过
    if (inTextInput(e.target)) {
      ;(e.target as HTMLElement).blur()
      return
    }
    skip()
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown, { capture: true }))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown, { capture: true } as any))
</script>

<template>
  <SessionInteractionCard
    role="dialog"
    :aria-label="$t('permission.question.title')"
  >
    <!-- 头部 -->
    <div class="flex items-center gap-2 px-3 py-2 border-b border-border">
      <span class="i-carbon-help w-4 h-4 text-primary shrink-0" aria-hidden="true" />
      <span class="text-sm font-medium text-foreground">{{ $t('permission.question.title') }}</span>
      <span class="text-xs text-muted-foreground">{{ $t('permission.question.subtitle') }}</span>
    </div>

    <!-- 问题组 -->
    <div class="px-3 py-2 max-h-96 overflow-y-auto space-y-3">
      <div v-for="(q, qi) in questions" :key="qi">
        <div class="flex items-center gap-1.5 flex-wrap text-xs">
          <span v-if="q.header" class="px-1.5 py-0.5 rounded border border-border text-muted-foreground shrink-0">{{ q.header }}</span>
          <span class="text-foreground font-medium">{{ q.question }}</span>
          <span v-if="q.multiSelect" class="text-muted-foreground shrink-0">{{ $t('permission.question.multiSelect') }}</span>
        </div>

        <div class="mt-1.5 space-y-1">
          <button
            v-for="(opt, oi) in q.options"
            :key="oi"
            type="button"
            class="option-row"
            :class="{ 'option-row-active': isPicked(qi, oi) }"
            @click="toggle(qi, oi)"
          >
            <span
              class="w-3.5 h-3.5 shrink-0 translate-y-0.5"
              :class="q.multiSelect
                ? (isPicked(qi, oi) ? 'i-carbon-checkbox-checked-filled text-primary' : 'i-carbon-checkbox text-muted-foreground')
                : (isPicked(qi, oi) ? 'i-carbon-radio-button-checked text-primary' : 'i-carbon-radio-button text-muted-foreground')"
              aria-hidden="true"
            />
            <span class="text-left">
              <span class="text-foreground font-medium">{{ opt.label }}</span>
              <span v-if="opt.description" class="text-muted-foreground"> — {{ opt.description }}</span>
            </span>
          </button>

          <!-- 「其他」自定义输入 -->
          <button
            type="button"
            class="option-row"
            :class="{ 'option-row-active': isPicked(qi, OTHER) }"
            @click="toggle(qi, OTHER)"
          >
            <span
              class="w-3.5 h-3.5 shrink-0 translate-y-0.5"
              :class="q.multiSelect
                ? (isPicked(qi, OTHER) ? 'i-carbon-checkbox-checked-filled text-primary' : 'i-carbon-checkbox text-muted-foreground')
                : (isPicked(qi, OTHER) ? 'i-carbon-radio-button-checked text-primary' : 'i-carbon-radio-button text-muted-foreground')"
              aria-hidden="true"
            />
            <span class="text-muted-foreground">{{ $t('permission.question.otherOption') }}</span>
          </button>
          <input
            v-if="isPicked(qi, OTHER)"
            :ref="el => setOtherInput(qi, el)"
            v-model="otherTexts[qi]"
            type="text"
            class="other-input"
            :placeholder="$t('permission.question.customPlaceholder')"
          />
        </div>
      </div>
    </div>

    <!-- 底部按钮 -->
    <div class="flex items-center gap-2 px-3 py-2 border-t border-border">
      <button
        type="button"
        class="btn btn-primary"
        :disabled="!allAnswered"
        @click="submit"
      >
        <span class="i-carbon-send w-3.5 h-3.5" aria-hidden="true" />
        {{ $t('permission.question.submit') }}
      </button>
      <span v-if="!allAnswered" class="text-xs text-muted-foreground">{{ $t('permission.question.allRequired') }}</span>
      <div class="flex-1" />
      <button type="button" class="btn btn-ghost" @click="skip">
        <span class="i-carbon-close w-3.5 h-3.5" aria-hidden="true" />
        {{ $t('permission.question.skip') }}
      </button>
    </div>
  </SessionInteractionCard>
</template>

<style scoped>
.question-card {
  min-width: 320px;
  max-width: 640px;
  width: 100%;
}

.option-row {
  display: flex;
  gap: 6px;
  align-items: baseline;
  width: 100%;
  padding: 4px 8px;
  border-radius: 4px;
  border: 1px solid transparent;
  font-size: 12px;
  line-height: 1.45;
  cursor: pointer;
  transition: background-color 100ms ease, border-color 100ms ease;
}

.option-row:hover {
  background-color: var(--muted);
}

.option-row-active {
  background-color: var(--secondary);
  border-color: var(--primary);
}

.other-input {
  width: 100%;
  margin-top: 2px;
  padding: 4px 8px;
  border: 1px solid var(--border);
  border-radius: 4px;
  background-color: var(--background);
  color: var(--foreground);
  font-size: 12px;
  outline: none;
}

.other-input:focus {
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

.btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.btn-primary {
  background-color: var(--primary);
  color: var(--primary-foreground);
}
.btn-primary:not(:disabled):hover {
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
  .btn,
  .option-row {
    transition: none !important;
  }
}
</style>

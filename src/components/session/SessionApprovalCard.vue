<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import SessionInteractionCard from './SessionInteractionCard.vue'

export interface SessionApprovalOption {
  id: string
  label: string
  tone: 'primary' | 'warn' | 'ghost'
  icon?: string
  title?: string
}

const props = withDefaults(defineProps<{
  title: string
  subject?: string | null
  danger?: boolean
  dangerReason?: string | null
  options: SessionApprovalOption[]
  defaultOptionId?: string | null
  denyOptionId?: string | null
  keyboard?: boolean
}>(), {
  subject: null,
  danger: false,
  dangerReason: null,
  defaultOptionId: null,
  denyOptionId: null,
  keyboard: true,
})

const emit = defineEmits<{
  (event: 'decide', decision: string): void
}>()

const root = ref<HTMLElement>()

function decide(decision: string) {
  emit('decide', decision)
}

function onKeydown(event: KeyboardEvent) {
  if (!props.keyboard) return
  if (event.key === 'Escape' && props.denyOptionId) {
    event.preventDefault()
    event.stopPropagation()
    decide(props.denyOptionId)
    return
  }
  if (event.key !== 'Enter' || !props.defaultOptionId) return
  const target = event.target as HTMLElement | null
  if (target?.closest('button')) return
  event.preventDefault()
  event.stopPropagation()
  decide(props.defaultOptionId)
}

onMounted(() => {
  if (!props.keyboard) return
  window.addEventListener('keydown', onKeydown, { capture: true })
  void nextTick(() => root.value?.querySelector<HTMLButtonElement>('[data-default-option="true"]')?.focus())
})

onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown, { capture: true } as AddEventListenerOptions))
</script>

<template>
  <div ref="root">
    <SessionInteractionCard
      :danger="danger"
      role="alertdialog"
      :aria-label="danger ? `${title}: ${dangerReason || subject || ''}` : title"
    >
      <div class="flex items-center gap-2 border-b border-border px-3 py-2">
        <span v-if="danger" class="i-carbon-warning-alt h-4 w-4 shrink-0 text-accent" aria-hidden="true" />
        <span v-else class="i-carbon-locked h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="text-xs text-muted-foreground">{{ title }}</span>
            <span v-if="subject" class="truncate text-sm font-medium text-foreground" :title="subject">{{ subject }}</span>
            <span v-if="danger" class="shrink-0 rounded border border-accent/50 px-1.5 py-0.5 text-[10px] font-medium text-accent">
              {{ $t('permission.highRiskBadge') }}
            </span>
          </div>
          <div v-if="danger && dangerReason" class="mt-0.5 truncate text-[10px] text-accent/90" :title="dangerReason">
            {{ dangerReason }}
          </div>
        </div>
      </div>

      <slot name="hint" />

      <div class="max-h-72 overflow-y-auto px-3 pb-2 pt-1">
        <slot />
      </div>

      <div class="flex items-center gap-2 border-t border-border px-3 py-2">
        <template v-for="(option, index) in options" :key="option.id">
          <div v-if="index > 0 && option.tone === 'ghost' && options[index - 1]?.tone !== 'ghost'" class="flex-1" />
          <button
            type="button"
            class="approval-button"
            :class="`approval-button-${option.tone}`"
            :title="option.title"
            :data-default-option="option.id === defaultOptionId"
            @click="decide(option.id)"
          >
            <span v-if="option.icon" class="h-3.5 w-3.5" :class="option.icon" aria-hidden="true" />
            {{ option.label }}
          </button>
        </template>
      </div>
    </SessionInteractionCard>
  </div>
</template>

<style scoped>
.approval-button {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border: 1px solid transparent;
  border-radius: 4px;
  outline: none;
  font-size: 12px;
  line-height: 1.4;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 120ms ease, color 120ms ease, box-shadow 120ms ease;
}

.approval-button:focus-visible { box-shadow: 0 0 0 2px var(--ring); }
.approval-button-primary { background: var(--primary); color: var(--primary-foreground); }
.approval-button-primary:hover { box-shadow: var(--shadow-paper); }
.approval-button-warn { border-color: var(--primary); color: var(--primary); }
.approval-button-warn:hover { background: var(--secondary); }
.approval-button-ghost { border-color: var(--border); color: var(--muted-foreground); }
.approval-button-ghost:hover { background: var(--muted); }

@media (prefers-reduced-motion: reduce) {
  .approval-button { transition: none; }
}
</style>

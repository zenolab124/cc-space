<script setup lang="ts">
import { computed } from 'vue'
import { resolveComposerAction } from './composerAction'

const props = withDefaults(defineProps<{
  dragging?: boolean
  busy?: boolean
  hasContent?: boolean
  canSendWhileBusy?: boolean
  sendDisabled?: boolean
  stopDisabled?: boolean
  stopLoading?: boolean
  stopVariant?: 'accent' | 'danger'
  sendLabel?: string
  stopLabel?: string
}>(), {
  dragging: false,
  busy: false,
  hasContent: false,
  canSendWhileBusy: false,
  sendDisabled: false,
  stopDisabled: false,
  stopLoading: false,
  stopVariant: 'accent',
  sendLabel: '',
  stopLabel: '',
})

const emit = defineEmits<{
  (event: 'send'): void
  (event: 'stop'): void
}>()

const action = computed(() => resolveComposerAction({
  busy: props.busy,
  hasContent: props.hasContent,
  canSendWhileBusy: props.canSendWhileBusy,
}))

const fieldClass = `min-h-9 flex-1 px-3 py-2 text-sm rounded-md bg-popover border border-border
  text-foreground placeholder-muted-foreground resize-none overflow-x-hidden
  placeholder:[white-space:pre-wrap] focus:outline-none focus:border-ring transition-colors
  disabled:cursor-not-allowed disabled:opacity-50`

const primaryActionClass = `min-h-9 shrink-0 rounded-md bg-primary px-3 py-2 text-xs
  text-primary-foreground transition-shadow hover:shadow-paper
  disabled:cursor-not-allowed disabled:opacity-30`

const secondaryActionClass = `min-h-9 shrink-0 rounded-md border border-border px-3 py-2 text-xs
  text-muted-foreground transition-colors hover:bg-muted hover:text-foreground
  disabled:cursor-not-allowed disabled:opacity-40`

const dangerActionClass = `min-h-9 shrink-0 rounded-md border border-destructive/30 bg-destructive/10
  px-3 py-2 text-xs text-destructive transition-colors hover:bg-destructive/15
  disabled:cursor-not-allowed disabled:opacity-40`
</script>

<template>
  <div
    class="session-composer relative shrink-0 border-t border-border bg-card px-4 py-3 transition-colors"
    :class="dragging && 'ring-1 ring-primary/40 ring-inset bg-primary/5'"
  >
    <slot name="notices" />
    <slot name="overlay" />
    <slot name="queue" />
    <slot name="attachments" />

    <div class="flex items-end gap-2">
      <slot name="field" :field-class="fieldClass" />
      <div class="flex shrink-0 items-center gap-1.5">
        <slot
          name="actions"
          :primary-action-class="primaryActionClass"
          :secondary-action-class="secondaryActionClass"
          :danger-action-class="dangerActionClass"
        >
          <button
            v-if="action === 'send'"
            type="button"
            :class="primaryActionClass"
            :disabled="!hasContent || sendDisabled"
            @click="emit('send')"
          >
            {{ sendLabel || $t('common.send') }}
          </button>
          <button
            v-else
            type="button"
            :class="['flex items-center gap-1.5', stopVariant === 'danger'
              ? dangerActionClass
              : [secondaryActionClass, 'border-accent bg-accent text-accent-foreground']]"
            :disabled="stopDisabled"
            :aria-busy="stopLoading"
            @click="emit('stop')"
          >
            <span v-if="stopLoading" aria-hidden="true" class="i-carbon-circle-dash h-3 w-3 shrink-0 animate-spin" />
            {{ stopLabel || $t('common.stop') }}
          </button>
        </slot>
      </div>
    </div>
  </div>
</template>

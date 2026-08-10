<script setup lang="ts">
import { useConfirm, type ConfirmAction } from '@/composables/useConfirm'

const { visible, message, confirmLabel, actions, settle, settleMulti } = useConfirm()

const btnBase = 'px-3 py-1.5 text-xs rounded-md transition-all'
const btnCancel = `${btnBase} border border-border text-muted-foreground hover:text-foreground hover:bg-muted`

function actionClass(act: ConfirmAction) {
  if (act.style === 'destructive') return `${btnBase} bg-destructive text-destructive-foreground hover:opacity-90`
  if (act.style === 'success') return `${btnBase} bg-primary text-white hover:shadow-paper`
  return `${btnBase} border border-border text-muted-foreground hover:text-foreground hover:bg-muted`
}
</script>

<template>
  <div
    v-if="visible"
    class="fixed inset-0 z-70 grid place-items-center"
    style="background: rgba(70, 45, 20, 0.18)"
    @mousedown.self="actions.length ? settleMulti(null) : settle(false)"
  >
    <div
      class="w-80 rounded bg-popover border border-border shadow-paper-lifted p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-dialog-message"
    >
      <p id="confirm-dialog-message" class="text-sm text-foreground whitespace-pre-wrap">{{ message }}</p>
      <div v-if="actions.length" class="mt-4 flex justify-end gap-2">
        <button type="button" :class="btnCancel" autofocus @click="settleMulti(null)">
          {{ $t('common.cancel') }}
        </button>
        <button
          v-for="act in actions"
          :key="act.value"
          type="button"
          :class="actionClass(act)"
          @click="settleMulti(act.value)"
        >
          {{ act.label }}
        </button>
      </div>
      <div v-else class="mt-4 flex justify-end gap-2">
        <button type="button" :class="btnCancel" autofocus @click="settle(false)">
          {{ $t('common.cancel') }}
        </button>
        <button
          type="button"
          :class="`${btnBase} bg-primary text-primary-foreground hover:shadow-paper`"
          @click="settle(true)"
        >
          {{ confirmLabel }}
        </button>
      </div>
    </div>
  </div>
</template>

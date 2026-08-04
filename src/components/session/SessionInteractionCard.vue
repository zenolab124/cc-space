<script setup lang="ts">
withDefaults(defineProps<{
  danger?: boolean
  role?: 'dialog' | 'alertdialog'
  ariaLabel?: string
}>(), {
  danger: false,
  role: 'dialog',
  ariaLabel: undefined,
})
</script>

<template>
  <div
    class="session-interaction-card w-full min-w-80 max-w-160 rounded-md border bg-popover shadow-paper-lifted"
    :class="danger ? 'border-accent/60 ring-1 ring-accent/25' : 'border-border'"
    :role="role"
    :aria-label="ariaLabel"
  >
    <slot />
  </div>
</template>

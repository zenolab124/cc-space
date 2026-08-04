<script setup lang="ts">
withDefaults(defineProps<{
  expanded: boolean
  state?: string
  title?: string
}>(), {
  state: 'neutral',
  title: undefined,
})

defineEmits<{ (event: 'toggle', value: MouseEvent): void }>()
</script>

<template>
  <div class="session-process-disclosure">
    <button
      type="button"
      class="session-process-line"
      :class="[`is-${state}`, { 'is-expanded': expanded }]"
      :aria-expanded="expanded"
      :title="title"
      @click="$emit('toggle', $event)"
    >
      <span class="i-carbon-chevron-right h-2.75 w-2.75 shrink-0 transition-transform" :class="expanded && 'rotate-90'" />
      <slot name="summary" />
      <span class="min-w-0 flex-1" />
      <slot name="status" />
    </button>
    <div v-if="expanded" class="session-process-detail">
      <slot />
    </div>
  </div>
</template>

<style scoped>
.session-process-disclosure {
  min-width: 0;
  margin: 0;
}
.session-process-line {
  display: flex;
  width: 100%;
  min-height: var(--tool-row-height);
  align-items: center;
  gap: 5px;
  border: 0;
  padding: 0;
  color: color-mix(in srgb, var(--muted-foreground) 62%, transparent);
  background: transparent;
  text-align: left;
  cursor: pointer;
  font-size: 10.5px;
  line-height: var(--tool-row-line-height);
  transition: color 120ms ease;
}
.session-process-line:hover,
.session-process-line:focus-visible,
.session-process-line.is-expanded,
.session-process-line.is-running,
.session-process-line.is-permission,
.session-process-line.is-error,
.session-process-line.is-failed,
.session-process-line.is-interrupted {
  color: var(--foreground);
}
.session-process-line:focus-visible {
  outline: 1px solid var(--ring);
  outline-offset: 2px;
}
.session-process-detail {
  margin: 0 0 4px 15px;
  padding: 2px 0 2px 9px;
  border-left: 1px solid var(--border);
}
</style>

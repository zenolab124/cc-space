<script setup lang="ts">
withDefaults(defineProps<{
  live?: 'polite' | 'assertive' | 'off'
}>(), {
  live: 'polite',
})
</script>

<template>
  <section
    class="session-interaction-panel shrink-0 flex justify-center border-t border-border bg-card px-4 py-2"
    :aria-live="live"
  >
    <div class="w-full max-w-160 space-y-2">
      <slot />
    </div>
  </section>
</template>

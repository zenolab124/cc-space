<script setup lang="ts">
defineProps<{ active: boolean }>()
</script>

<template>
  <div
    class="session-typing-indicator absolute bottom-1 left-0 flex items-center gap-1 pl-5 transition-opacity duration-200"
    :class="active ? 'opacity-100' : 'opacity-0 pointer-events-none'"
    aria-hidden="true"
  >
    <template v-if="active">
      <span class="session-typing-dot" />
      <span class="session-typing-dot" />
      <span class="session-typing-dot" />
    </template>
  </div>
</template>

<style scoped>
.session-typing-dot {
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: var(--muted-foreground);
  opacity: 0.4;
  animation: session-typing-blink 1.4s infinite both;
}
.session-typing-dot:nth-child(2) { animation-delay: 0.2s; }
.session-typing-dot:nth-child(3) { animation-delay: 0.4s; }
@keyframes session-typing-blink {
  0%, 80%, 100% { opacity: 0.15; }
  40% { opacity: 0.6; }
}
@media (prefers-reduced-motion: reduce) {
  .session-typing-dot { animation: none; opacity: 0.5; }
}
</style>

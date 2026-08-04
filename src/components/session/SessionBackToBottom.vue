<script setup lang="ts">
defineEmits<{ (event: 'click'): void }>()
</script>

<template>
  <div class="sticky bottom-0 flex justify-center pointer-events-none">
    <button
      type="button"
      class="pointer-events-auto flex items-center gap-1 rounded-full border border-border bg-popover px-3 py-1 text-xs text-muted-foreground shadow-paper transition-colors hover:text-foreground"
      @click="$emit('click')"
    >
      <span class="i-carbon-arrow-down h-3 w-3" />
      {{ $t('session.backToBottom') }}
    </button>
  </div>
</template>

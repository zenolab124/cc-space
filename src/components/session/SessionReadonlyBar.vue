<script setup lang="ts">
defineProps<{ label: string }>()
</script>

<template>
  <div class="session-readonly-bar shrink-0 flex flex-wrap items-center gap-2 border-t border-border bg-card px-4 py-2 text-xs text-muted-foreground">
    <div class="flex min-w-0 flex-1 items-center gap-2">
      <span class="i-carbon-document h-3.5 w-3.5 shrink-0" />
      <span class="min-w-0 flex-1 truncate">{{ label }}</span>
    </div>
    <div class="flex min-w-0 max-w-full flex-wrap items-center justify-end gap-2">
      <slot />
    </div>
  </div>
</template>

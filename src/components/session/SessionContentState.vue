<script setup lang="ts">
withDefaults(defineProps<{
  tone?: 'muted' | 'danger'
  fill?: boolean
}>(), {
  tone: 'muted',
  fill: false,
})
</script>

<template>
  <div
    class="session-content-state text-center text-sm"
    :class="[
      fill ? 'flex flex-1 items-center justify-center' : 'py-10',
      tone === 'danger' ? 'text-destructive' : 'text-muted-foreground',
    ]"
    :role="tone === 'danger' ? 'alert' : undefined"
  >
    <slot />
  </div>
</template>

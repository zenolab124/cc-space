<script setup lang="ts">
import MsgClamp from '@/components/MsgClamp.vue'

defineProps<{
  timeLabel?: string | null
}>()
</script>

<template>
  <div class="conversation-user-message flex gap-3">
    <div class="w-0.5 shrink-0 rounded-full bg-primary/60" aria-hidden="true" />
    <div class="min-w-0 flex-1 rounded border border-border bg-card px-3 py-2 shadow-paper">
      <div class="mb-1 flex items-baseline gap-2 text-xs font-medium text-primary">
        <span>{{ $t('session.you') }}</span>
        <span
          v-if="timeLabel"
          class="font-normal tabular-nums text-muted-foreground/60"
        >{{ timeLabel }}</span>
        <span class="min-w-0 flex-1" />
        <slot name="actions" />
      </div>
      <MsgClamp>
        <slot />
      </MsgClamp>
    </div>
  </div>
</template>

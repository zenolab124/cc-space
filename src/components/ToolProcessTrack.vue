<script setup lang="ts">
import type { ContentBlock } from '@/types'
import type { ToolUseBlock } from '@/utils/toolDisplay'
import ToolImageStack from './ToolImageStack.vue'
import ToolProcessGroup from './ToolProcessGroup.vue'

defineProps<{
  blocks: ContentBlock[]
  tools: ToolUseBlock[]
  streaming?: boolean
}>()
</script>

<template>
  <div class="tool-process-track">
    <div class="tool-process-track-layout">
      <div class="tool-process-track-main">
        <ToolProcessGroup
          :blocks="blocks"
          :tools="tools"
          :streaming="streaming"
          latest-only
          :show-images="false"
        />
      </div>
      <ToolImageStack :tools="tools" />
    </div>
  </div>
</template>

<style scoped>
.tool-process-track {
  min-width: 0;
  container-type: inline-size;
}
.tool-process-track-layout {
  display: grid;
  min-width: 0;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  border: 1px solid color-mix(in srgb, var(--border) 62%, transparent);
  border-radius: 6px;
  background: linear-gradient(
    105deg,
    color-mix(in srgb, var(--card) 76%, transparent),
    color-mix(in srgb, var(--muted) 32%, transparent)
  );
  box-shadow: 0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent);
}
.tool-process-track-main {
  min-width: 0;
  padding: 5px 8px 5px 7px;
}
@container (max-width: 420px) {
  .tool-process-track-main { padding-right: 4px; }
}
</style>

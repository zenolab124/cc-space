<script setup lang="ts">
defineProps<{
  contentId: string
  text: string
  expanded: boolean
  isError?: boolean
}>()

const emit = defineEmits<{ (event: 'toggle', value: MouseEvent): void }>()
</script>

<template>
  <div
    :id="contentId"
    class="tool-result-preview-card"
    :class="{ 'is-expanded': expanded, 'is-error': isError }"
  >
    <button
      v-if="text"
      type="button"
      class="tool-result-preview-toggle"
      :aria-expanded="expanded"
      :aria-label="$t(expanded ? 'common.collapse' : 'common.expand')"
      :title="$t(expanded ? 'common.collapse' : 'common.expand')"
      @click="emit('toggle', $event)"
    >
      <pre
        class="tool-result-preview-text"
        :class="expanded ? '' : 'tool-result-clamp-3'"
      >{{ text }}</pre>
      <span
        class="tool-result-preview-cue"
        :class="expanded ? 'i-carbon-chevron-up' : 'i-carbon-chevron-down'"
        aria-hidden="true"
      />
    </button>
  </div>
</template>

<style scoped>
.tool-result-preview-card {
  min-width: 0;
  margin: 3px 0 7px 18px;
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--border) 82%, transparent);
  border-radius: 6px;
  color: var(--muted-foreground);
  background: color-mix(in srgb, var(--card) 72%, transparent);
  box-shadow: 0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent);
  transition: border-color 120ms ease, background-color 120ms ease;
}
.tool-result-preview-card:hover,
.tool-result-preview-card:focus-within {
  border-color: color-mix(in srgb, var(--primary) 28%, var(--border));
  background: color-mix(in srgb, var(--card) 90%, transparent);
}
.tool-result-preview-card.is-error {
  border-color: color-mix(in srgb, var(--destructive) 28%, var(--border));
  color: var(--destructive);
}
.tool-result-preview-toggle {
  position: relative;
  display: block;
  width: 100%;
  border: 0;
  padding: 6px 28px 7px 9px;
  color: inherit;
  background: transparent;
  text-align: left;
  cursor: pointer;
}
.tool-result-preview-toggle:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: -2px;
}
.tool-result-preview-text {
  min-width: 0;
  margin: 0;
  overflow-wrap: anywhere;
  font-family: inherit;
  font-size: 11.5px;
  line-height: 1.5;
  white-space: pre-wrap;
}
.tool-result-clamp-3 {
  display: -webkit-box;
  overflow: hidden;
  -webkit-box-orient: vertical;
}
.tool-result-clamp-3 { -webkit-line-clamp: 3; }
.tool-result-preview-cue {
  position: absolute;
  right: 8px;
  bottom: 8px;
  width: 11px;
  height: 11px;
  color: color-mix(in srgb, currentColor 58%, transparent);
}
</style>

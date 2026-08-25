<script setup lang="ts">
import { computed, ref } from 'vue'
import { buildTextDiff, foldTextDiffRows } from '@/utils/textDiff'

const props = withDefaults(defineProps<{
  oldText?: string | null
  newText?: string | null
  unifiedDiff?: string | null
  compact?: boolean
}>(), {
  oldText: '',
  newText: '',
  unifiedDiff: '',
  compact: false,
})

const expanded = ref(false)
const model = computed(() => buildTextDiff(props))
const foldedRows = computed(() => foldTextDiffRows(model.value.rows))
const hasFold = computed(() => foldedRows.value.some(row => row.kind === 'fold'))
const visibleRows = computed(() => expanded.value ? model.value.rows : foldedRows.value)
</script>

<template>
  <div class="text-diff-viewer" :class="{ 'is-compact': compact }">
    <div class="text-diff-toolbar">
      <span class="diff-stat diff-stat-add">+{{ model.stats.additions }}</span>
      <span class="diff-stat diff-stat-delete">−{{ model.stats.deletions }}</span>
      <button
        v-if="hasFold"
        type="button"
        class="ml-auto text-[10px] text-primary hover:underline"
        @click="expanded = !expanded"
      >{{ expanded ? $t('diff.collapseContext') : $t('diff.expandAll') }}</button>
    </div>
    <div class="text-diff-scroll" tabindex="0">
      <div
        v-for="(row, index) in visibleRows"
        :key="`${index}:${row.kind}`"
        class="text-diff-row"
        :class="`is-${row.kind}`"
      >
        <template v-if="row.kind === 'fold'">
          <button
            type="button"
            class="text-diff-fold"
            @click="expanded = true"
          >{{ $t('diff.hiddenContext', { count: row.hiddenCount }) }}</button>
        </template>
        <template v-else>
          <span class="text-diff-marker" aria-hidden="true">{{ row.kind === 'add' ? '+' : row.kind === 'delete' ? '−' : ' ' }}</span>
          <span class="text-diff-number">{{ row.oldNumber ?? '' }}</span>
          <span class="text-diff-number">{{ row.newNumber ?? '' }}</span>
          <code class="text-diff-code"><template v-if="row.chunks"><span
            v-for="(chunk, chunkIndex) in row.chunks"
            :key="chunkIndex"
            :class="chunk.kind !== 'context' && `is-${chunk.kind}`"
          >{{ chunk.text }}</span></template><template v-else>{{ row.text }}</template></code>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.text-diff-viewer {
  min-width: 0;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: var(--background);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11px;
  line-height: 1.5;
}
.text-diff-toolbar {
  display: flex;
  min-height: 26px;
  align-items: center;
  gap: 8px;
  padding: 3px 8px;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--muted) 55%, transparent);
}
.diff-stat { font-weight: 650; font-variant-numeric: tabular-nums; }
.diff-stat-add { color: var(--primary); }
.diff-stat-delete { color: var(--destructive); }
.text-diff-scroll {
  max-height: 288px;
  overflow: auto;
  outline: none;
}
.text-diff-scroll:focus-visible { box-shadow: inset 0 0 0 2px var(--ring); }
.text-diff-row {
  display: grid;
  grid-template-columns: 16px 34px 34px minmax(max-content, 1fr);
  min-width: max-content;
}
.text-diff-row.is-add { background: color-mix(in srgb, var(--primary) 10%, transparent); }
.text-diff-row.is-delete { background: color-mix(in srgb, var(--destructive) 9%, transparent); }
.text-diff-marker,
.text-diff-number {
  user-select: none;
  text-align: right;
  color: var(--muted-foreground);
}
.text-diff-marker { padding-right: 4px; font-weight: 700; }
.is-add > .text-diff-marker { color: var(--primary); }
.is-delete > .text-diff-marker { color: var(--destructive); }
.text-diff-number {
  padding: 0 6px;
  border-left: 1px solid color-mix(in srgb, var(--border) 65%, transparent);
  font-variant-numeric: tabular-nums;
}
.text-diff-code {
  display: block;
  min-width: 0;
  padding: 0 8px;
  color: var(--foreground);
  white-space: pre;
}
.text-diff-code .is-add {
  border-radius: 2px;
  background: color-mix(in srgb, var(--primary) 28%, transparent);
}
.text-diff-code .is-delete {
  border-radius: 2px;
  background: color-mix(in srgb, var(--destructive) 25%, transparent);
}
.text-diff-fold {
  grid-column: 1 / -1;
  width: 100%;
  padding: 2px 8px;
  border: 0;
  color: var(--muted-foreground);
  background: color-mix(in srgb, var(--muted) 45%, transparent);
  text-align: center;
  cursor: pointer;
}
.text-diff-fold:hover { color: var(--foreground); background: var(--muted); }
.is-compact { font-size: 10.5px; }
.is-compact .text-diff-scroll { max-height: 220px; }
</style>

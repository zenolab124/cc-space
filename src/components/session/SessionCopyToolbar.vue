<script setup lang="ts">
import type { SemanticCopyMode } from '@/utils/semanticClipboard'

defineProps<{
  visible: boolean
  menuOpen: boolean
  left: number
  top: number
}>()

const emit = defineEmits<{
  copy: [mode: SemanticCopyMode]
  toggleMenu: []
}>()

const copyModes: SemanticCopyMode[] = ['plain', 'markdown', 'rich', 'full']
</script>

<template>
  <Teleport to="body">
    <div
      v-if="visible"
      data-copy-exclude
      class="fixed z-950 flex rounded-md border border-border bg-card text-foreground shadow-paper"
      :style="{ left: `${left}px`, top: `${top}px` }"
      @mousedown.prevent
    >
      <button
        type="button"
        class="h-7 px-2.5 text-xs font-medium hover:bg-muted"
        :title="$t('copy.richDescription')"
        @click="emit('copy', 'rich')"
      >
        {{ $t('copy.rich') }}
      </button>
      <button
        type="button"
        class="flex h-7 w-7 items-center justify-center border-l border-border hover:bg-muted"
        :aria-label="$t('copy.chooseMode')"
        :aria-expanded="menuOpen"
        @click="emit('toggleMenu')"
      >
        <span class="i-carbon-chevron-down h-3 w-3" aria-hidden="true" />
      </button>
      <div
        v-if="menuOpen"
        class="absolute right-0 top-[calc(100%+4px)] min-w-42 overflow-hidden rounded-md border border-border bg-card py-1 shadow-paper"
      >
        <button
          v-for="mode in copyModes"
          :key="mode"
          type="button"
          class="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs hover:bg-muted"
          :title="mode === 'full' ? $t('copy.fullDescription') : undefined"
          @click="emit('copy', mode)"
        >
          <span :class="mode === 'rich' ? 'i-carbon-checkmark' : ''" class="h-3 w-3 shrink-0" />
          {{ $t(`copy.${mode}`) }}
        </button>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
defineProps<{
  title: string
  description: string
  choices: Array<{
    id: string
    label: string
    description: string
    icon: string
    accent: string
    available: boolean
    checking: boolean
    current?: boolean
  }>
  selectingEngine: string | null
  selectingHint?: string | null
  error?: string | null
}>()

const emit = defineEmits<{
  (e: 'select', engineId: string): void
}>()
</script>

<template>
  <section class="engine-picker h-full overflow-y-auto bg-background/35 px-5 py-8 flex items-center justify-center">
    <div class="w-full max-w-2xl text-center">
      <div class="mx-auto mb-3 h-9 w-9 rounded border border-border bg-card shadow-paper flex items-center justify-center text-primary">
        <span class="i-carbon-rocket h-4.5 w-4.5" />
      </div>
      <h2 class="text-base font-semibold text-foreground">{{ title }}</h2>
      <p class="mt-1 text-xs leading-relaxed text-muted-foreground">{{ description }}</p>

      <div class="engine-choice-grid mt-5" role="group" :aria-label="title">
        <button
          v-for="choice in choices"
          :key="choice.id"
          type="button"
          class="engine-choice group"
          :class="{ 'engine-choice-current': choice.current }"
          :style="{ '--engine-accent': `var(--${choice.accent})` }"
          :disabled="!choice.available || selectingEngine !== null"
          :aria-busy="selectingEngine === choice.id"
          :aria-pressed="choice.current || undefined"
          @click="emit('select', choice.id)"
        >
          <span class="engine-choice-icon">
            <span :class="[choice.icon, 'h-5 w-5']" />
          </span>
          <span class="min-w-0 text-left">
            <span class="block text-sm font-semibold text-foreground">{{ choice.label }}</span>
            <span class="mt-1 block text-[11px] leading-relaxed text-muted-foreground">
              {{ choice.description }}
            </span>
            <span v-if="choice.checking" class="mt-2 block text-[10px] text-muted-foreground">
              {{ $t('common.loading') }}
            </span>
            <span v-else-if="!choice.available" class="mt-2 block text-[10px] text-destructive">
              {{ $t('workbench.enginePicker.unavailable') }}
            </span>
            <span v-else-if="selectingEngine === choice.id" class="mt-2 block text-[10px] text-primary">
              <span role="status">{{ selectingHint || $t('workbench.enginePicker.starting') }}</span>
            </span>
            <span v-else-if="choice.current" class="mt-2 block text-[10px] text-primary">
              {{ $t('workbench.enginePicker.current') }}
            </span>
          </span>
          <span class="i-carbon-arrow-right ml-auto h-4 w-4 shrink-0 opacity-45 transition-transform group-hover:translate-x-0.5" />
        </button>
      </div>

      <p v-if="error" class="mt-4 text-xs text-destructive" role="alert">{{ error }}</p>
    </div>
  </section>
</template>

<style scoped>
.engine-picker {
  container-type: inline-size;
}
.engine-choice-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 0.75rem;
}
.engine-choice {
  display: flex;
  width: 100%;
  min-width: 0;
  min-height: 112px;
  align-items: center;
  gap: 0.75rem;
  padding: 1rem;
  border: 1px solid color-mix(in srgb, var(--engine-accent) 28%, var(--border));
  border-radius: var(--radius);
  background: var(--card);
  box-shadow: var(--shadow-paper);
  text-align: left;
  transition: transform 150ms ease, box-shadow 150ms ease, border-color 150ms ease;
}
.engine-choice-current {
  border-color: color-mix(in srgb, var(--engine-accent) 52%, var(--border));
  background: color-mix(in srgb, var(--engine-accent) 5%, var(--card));
}
.engine-choice:hover:not(:disabled) {
  transform: translateY(-2px);
  border-color: color-mix(in srgb, var(--engine-accent) 55%, var(--border));
  box-shadow: var(--shadow-paper-lifted);
}
.engine-choice:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
}
.engine-choice:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
.engine-choice-icon {
  display: inline-flex;
  width: 38px;
  height: 38px;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  border: 1px solid color-mix(in srgb, var(--engine-accent) 28%, transparent);
  border-radius: var(--radius);
  color: var(--engine-accent);
  background: color-mix(in srgb, var(--engine-accent) 9%, var(--card));
}
@container (min-width: 480px) {
  .engine-choice-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>

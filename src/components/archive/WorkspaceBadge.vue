<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { WorkspaceContext } from '@/types'

const props = defineProps<{ context: WorkspaceContext }>()
const { t } = useI18n()

const legacy = computed(() => props.context.kind === 'legacy' && !props.context.available)
const label = computed(() => legacy.value
  ? t('worktreeSession.deleted')
  : props.context.branch || props.context.name || t('worktreeSession.linked'))
const tooltip = computed(() => legacy.value
  ? t('worktreeSession.deletedTooltip', { path: props.context.worktreeRoot })
  : t('worktreeSession.linkedTooltip', {
      name: props.context.name || '—',
      branch: props.context.branch || '—',
      path: props.context.worktreeRoot,
    }))
</script>

<template>
  <span
    v-if="context.kind !== 'primary'"
    class="inline-flex max-w-36 shrink-0 items-center gap-1 rounded border px-1.5 py-0.5 text-[9px] font-medium"
    :class="legacy
      ? 'border-accent/30 bg-accent/10 text-accent'
      : 'border-border bg-muted text-muted-foreground'"
    :title="tooltip"
  >
    <span :class="legacy ? 'i-carbon-warning-alt' : 'i-carbon-branch'" class="h-3 w-3 shrink-0" aria-hidden="true" />
    <span class="truncate">{{ label }}</span>
  </span>
</template>

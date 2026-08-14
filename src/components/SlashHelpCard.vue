<script setup lang="ts">
import type { SlashCommand } from '@/composables/useSlashCommands'
import { useI18n } from 'vue-i18n'

defineProps<{
  commands: SlashCommand[]
}>()

const { t } = useI18n()

function badgeText(cat: string): string {
  if (cat === 'skill') return t('slash.badgeSkill')
  if (cat === 'command') return t('slash.badgeCommand')
  if (cat === 'terminal') return t('slash.badgeTerminal')
  if (cat === 'pass') return t('slash.passThrough')
  return t('slash.native')
}

function badgeTitle(cat: string): string {
  if (cat === 'skill' || cat === 'command') return ''
  if (cat === 'terminal') return t('slash.terminalTitle')
  if (cat === 'pass') return t('slash.passThroughTitle')
  return t('slash.nativeTitle')
}
</script>

<template>
  <div class="mt-2 rounded-md border border-border bg-popover/40 px-3 py-2">
    <div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
      <span class="i-carbon-help w-3.5 h-3.5 shrink-0" />
      <span>{{ $t('slash.title') }}</span>
      <span class="text-muted-foreground font-normal">{{ $t('slash.totalCount', { count: commands.length }) }}</span>
    </div>

    <ul class="mt-2 space-y-1">
      <li
        v-for="cmd in commands"
        :key="cmd.name"
        class="flex items-baseline gap-2 text-xs"
      >
        <span class="font-mono text-primary shrink-0">{{ cmd.wirePrefix }}{{ cmd.name }}</span>
        <span
          v-if="cmd.hasArg && cmd.argHint"
          class="font-mono text-muted-foreground shrink-0"
        >
          {{ cmd.argHint }}
        </span>
        <span class="text-muted-foreground break-words">{{ cmd.hint }}</span>
        <span
          class="ml-auto px-1.5 py-0.5 rounded text-2xs text-muted-foreground border border-border shrink-0"
          :title="badgeTitle(cmd.category)"
        >
          {{ badgeText(cmd.category) }}
        </span>
      </li>
    </ul>

    <div class="mt-2 pt-2 border-t border-border text-2xs text-muted-foreground">
      {{ $t('slash.footer') }}
    </div>
  </div>
</template>

<style scoped>
.text-2xs {
  font-size: 10px;
  line-height: 1.3;
}
</style>

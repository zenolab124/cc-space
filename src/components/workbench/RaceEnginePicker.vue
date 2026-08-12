<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { instanceKey } from '@/engines/identity'
import { useEngines } from '@/engines/useEngines'
import EngineChoicePanel from './EngineChoicePanel.vue'

const props = defineProps<{
  currentEngineId: string
  selectingEngineId: string | null
}>()

const emit = defineEmits<{
  (e: 'select', engineId: string): void
}>()

type TargetEngineId = 'claude-code' | 'codex'

const { t } = useI18n()
const { engines, health, loading, errors, refreshEngines } = useEngines()

const targets: Array<{ id: TargetEngineId; label: string; icon: string; accent: 'claude' | 'codex' }> = [
  { id: 'claude-code', label: 'Claude Code', icon: 'i-simple-anthropic', accent: 'claude' },
  { id: 'codex', label: 'Codex', icon: 'i-simple-openai', accent: 'codex' },
]

const choices = computed(() => targets.map(target => {
  const descriptor = engines.value.find(item => item.instance.engineId === target.id) ?? null
  const runtimeHealth = descriptor ? health.value[instanceKey(descriptor.instance)]?.runtime : null
  const healthFailed = descriptor ? !!errors.value[instanceKey(descriptor.instance)] : false
  return {
    ...target,
    description: t(`workbench.enginePicker.${target.id === 'claude-code' ? 'claudeDescription' : 'codexDescription'}`),
    available: !!descriptor?.enabled
      && descriptor.capabilities.runtime?.create === true
      && runtimeHealth?.available === true,
    checking: loading.value || (!!descriptor && !runtimeHealth && !healthFailed),
    current: props.currentEngineId === target.id,
  }
}))

onMounted(() => {
  if (engines.value.length === 0) void refreshEngines()
})
</script>

<template>
  <EngineChoicePanel
    :title="t('workbench.race.chooseEngine')"
    :description="t('workbench.race.chooseEngineDescription')"
    :choices="choices"
    :selecting-engine="selectingEngineId"
    @select="emit('select', $event)"
  />
</template>

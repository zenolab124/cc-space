<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChannels, channelSupportsEngine, refreshChannels } from '@/composables/useChannels'
import { useNotifications } from '@/composables/useNotifications'
import { useEngineNotices } from '@/composables/useEngineNotices'
import { useProjects } from '@/composables/useProjects'
import { useWorkbench } from '@/composables/useWorkbench'
import { createSession } from '@/engines/client'
import { instanceKey, sameInstance } from '@/engines/identity'
import { sessionUiId, usesNativeSessionSurface } from '@/engines/integration'
import { useEngines } from '@/engines/useEngines'
import type { EngineDescriptor, ProjectRef } from '@/engines/types'
import type { Project } from '@/types'
import EngineChoicePanel from './EngineChoicePanel.vue'

const props = defineProps<{
  sessionId: string
  cwd: string
}>()

type TargetEngineId = 'claude-code' | 'codex'

const { t } = useI18n()
const { projects } = useProjects()
const { engines, health, loading, errors, refreshEngines } = useEngines()
const { channels, defaultSessionChannels } = useChannels()
const {
  promotePendingTaskToDraft,
  stageEngineDraft,
  replaceWorkbenchSession,
  discardStagedSession,
} = useWorkbench()
const { notifyTransient } = useNotifications()
const { codexCreateDelayRisk } = useEngineNotices()
const selectingEngine = ref<TargetEngineId | null>(null)
const selectingHint = ref<string | null>(null)
const error = ref<string | null>(null)
let slowHintTimer: ReturnType<typeof setTimeout> | null = null

const targets: Array<{ id: TargetEngineId; icon: string; accent: 'claude' | 'codex' }> = [
  { id: 'claude-code', icon: 'i-simple-anthropic', accent: 'claude' },
  { id: 'codex', icon: 'i-simple-openai', accent: 'codex' },
]

const choices = computed(() => targets.map(target => {
  const descriptor = engines.value.find(item => item.instance.engineId === target.id) ?? null
  const runtimeHealth = descriptor ? health.value[instanceKey(descriptor.instance)]?.runtime : null
  const healthFailed = descriptor ? !!errors.value[instanceKey(descriptor.instance)] : false
  return {
    ...target,
    label: target.id === 'claude-code' ? 'Claude Code' : 'Codex',
    description: t(`workbench.enginePicker.${target.id === 'claude-code' ? 'claudeDescription' : 'codexDescription'}`),
    descriptor,
    available: !!descriptor?.enabled
      && descriptor.capabilities.runtime?.create === true
      && runtimeHealth?.available === true,
    checking: loading.value || (!!descriptor && !runtimeHealth && !healthFailed),
  }
}))

function projectCwd(project: Project): string | null {
  return project.sessions.find(session => session.cwd)?.cwd ?? project.source_path ?? null
}

function projectReference(engine: EngineDescriptor): ProjectRef {
  return projects.value.find(project =>
    !!project.reference
    && sameInstance(project.reference.engine, engine.instance)
    && projectCwd(project) === props.cwd,
  )?.reference ?? { engine: engine.instance, nativeId: props.cwd }
}

function defaultChannelForEngine(engine: EngineDescriptor): string | null {
  const engineId = engine.instance.engineId
  if (engineId !== 'claude-code' && engineId !== 'codex') return null
  const channelId = defaultSessionChannels.value[engineId]
  if (!channelId) return null
  const channel = channels.value.find(item => item.id === channelId)
  return channel?.enabled
    && channel.scope !== 'agent-only'
    && channelSupportsEngine(channel, engineId)
    ? channelId
    : null
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause)
}

async function selectEngine(engineId: string) {
  const choice = choices.value.find(item => item.id === engineId)
  if (!choice) return
  if (!choice.available || !choice.descriptor || selectingEngine.value) return
  selectingEngine.value = choice.id
  error.value = null
  try {
    if (usesNativeSessionSurface(choice.descriptor.instance)) {
      if (!promotePendingTaskToDraft(props.sessionId)) throw new Error(t('workbench.enginePicker.expired'))
    } else {
      await refreshChannels()
      const attachedChannel = defaultChannelForEngine(choice.descriptor)
      const project = projectReference(choice.descriptor)
      if (choice.id === 'codex') {
        slowHintTimer = setTimeout(() => {
          selectingHint.value = t(codexCreateDelayRisk.value
            ? 'workbench.enginePicker.codexVersionRefresh'
            : 'workbench.enginePicker.codexSlowStart')
        }, 800)
      }
      const created = await createSession(project, props.cwd, attachedChannel ? { channelId: attachedChannel } : {})
      const replacementSessionId = sessionUiId(created.session)
      stageEngineDraft(replacementSessionId, {
        reference: created.session,
        project,
        engineName: choice.descriptor.displayName,
        cwd: props.cwd,
        attachedChannel,
        attachedCapabilityFingerprint: created.capabilityFingerprint,
      })
      if (!replaceWorkbenchSession(props.sessionId, replacementSessionId)) {
        discardStagedSession(replacementSessionId)
        throw new Error(t('workbench.enginePicker.expired'))
      }
    }
    notifyTransient(t('workbench.rail.newSessionReady'), t('workbench.rail.newSessionHint'))
  } catch (cause) {
    const message = errorMessage(cause)
    error.value = message
    notifyTransient(t('common.newSessionFailed'), message)
  } finally {
    if (slowHintTimer) clearTimeout(slowHintTimer)
    slowHintTimer = null
    selectingHint.value = null
    selectingEngine.value = null
  }
}

onMounted(() => {
  void refreshChannels()
  if (engines.value.length === 0) void refreshEngines()
})

onUnmounted(() => {
  if (slowHintTimer) clearTimeout(slowHintTimer)
})
</script>

<template>
  <EngineChoicePanel
    :title="t('workbench.enginePicker.title')"
    :description="t('workbench.enginePicker.description')"
    :choices="choices"
    :selecting-engine="selectingEngine"
    :selecting-hint="selectingHint"
    :error="error"
    @select="selectEngine"
  />
</template>

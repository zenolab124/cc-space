<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Menu } from '@tauri-apps/api/menu'
import { useI18n } from 'vue-i18n'
import type { SessionSummary } from '@/types'
import { listEngines, forkSession } from '@/engines/client'
import { sessionUiId } from '@/engines/integration'
import { engineRuntimeChannel, engineRuntimeOptions, inheritEngineRunConfig } from '@/engines/runConfig'
import { useWorkbench } from '@/composables/useWorkbench'
import { useWorkspaceContexts } from '@/composables/useWorkspaceContexts'
import { useUiState } from '@/composables/useUiState'
import { useNotifications } from '@/composables/useNotifications'
import { useProjects } from '@/composables/useProjects'

const props = defineProps<{ session: SessionSummary }>()
const { t } = useI18n()
const { workspaceForSession } = useWorkspaceContexts()
const { ordinaryTabs, defaultOrdinaryTab, stageEngineDraft, openSessionInTab } = useWorkbench()
const { switchSection } = useUiState()
const { notifyTransient } = useNotifications()
const { projects } = useProjects()
const continuing = ref(false)
const menuOpening = ref(false)
const runtimeSupportsFork = ref(false)
let capabilityGeneration = 0

const context = computed(() => workspaceForSession(props.session))
const visible = computed(() => context.value?.kind === 'legacy' && !context.value.available)
const canContinue = computed(() => visible.value
  && !!context.value?.mainRoot
  && context.value.mainAvailable
  && !!props.session.reference
  && !!props.session.project_reference
  && runtimeSupportsFork.value)
const reason = computed(() => {
  if (!context.value?.mainAvailable) return t('worktreeSession.mainUnavailable')
  if (!runtimeSupportsFork.value) return t('worktreeSession.continueUnsupported')
  return ''
})

watch(() => props.session.engine, async (engine) => {
  const generation = ++capabilityGeneration
  runtimeSupportsFork.value = false
  if (!engine) return
  try {
    const engines = await listEngines()
    const supported = engines.some(descriptor =>
      descriptor.enabled
      && descriptor.instance.engineId === engine.engineId
      && descriptor.instance.instanceId === engine.instanceId
      && descriptor.capabilities.runtime?.forkWithCwd === true)
    if (generation === capabilityGeneration) runtimeSupportsFork.value = supported
  } catch {
    if (generation === capabilityGeneration) runtimeSupportsFork.value = false
  }
}, { immediate: true, deep: true })

async function continueInMain(targetTabId?: string) {
  const workspace = context.value
  const reference = props.session.reference
  const project = props.session.project_reference
  if (!canContinue.value || !workspace?.mainRoot || !reference || !project || continuing.value) return
  const mainRoot = workspace.mainRoot
  continuing.value = true
  try {
    const attachedChannel = engineRuntimeChannel(props.session.id)
    const created = await forkSession(reference, null, {
      ...engineRuntimeOptions(props.session.id),
      cwd: mainRoot,
    })
    const sessionId = sessionUiId(created.session)
    const mainProject = projects.value.find(candidate =>
      candidate.reference
      && candidate.engine?.engineId === reference.engine.engineId
      && candidate.engine.instanceId === reference.engine.instanceId
      && sameWorkspacePath(candidate.source_path, mainRoot))
    stageEngineDraft(sessionId, {
      reference: created.session,
      project: mainProject?.reference ?? project,
      engineName: props.session.engine_name ?? reference.engine.engineId,
      cwd: mainRoot,
      sourceMeta: created.sourceMeta,
      attachedChannel,
      attachedCapabilityFingerprint: created.capabilityFingerprint,
    })
    inheritEngineRunConfig(props.session.id, sessionId)
    openSessionInTab(sessionId, targetTabId)
    switchSection('workbench')
  } catch (cause) {
    notifyTransient(t('worktreeSession.continueFailed'), String(cause))
  } finally {
    continuing.value = false
  }
}

function sameWorkspacePath(left: string | null | undefined, right: string): boolean {
  if (!left) return false
  const normalize = (value: string) => {
    const normalized = value.replace(/\\/g, '/').replace(/\/+$/, '')
    return /^[a-z]:\//i.test(normalized) || normalized.startsWith('//')
      ? normalized.toLowerCase()
      : normalized
  }
  return normalize(left) === normalize(right)
}

async function chooseTarget() {
  if (!canContinue.value || continuing.value || menuOpening.value || !ordinaryTabs.value.length) return
  menuOpening.value = true
  const defaultId = defaultOrdinaryTab.value?.id
  try {
    const menu = await Menu.new({
      items: ordinaryTabs.value.map(tab => ({
        text: tab.id === defaultId
          ? t('workbench.targetDefault', { name: tab.name })
          : tab.name,
        action: () => {
          menuOpening.value = false
          void continueInMain(tab.id)
        },
      })),
    })
    await menu.popup()
  } finally {
    menuOpening.value = false
  }
}
</script>

<template>
  <div v-if="visible" class="inline-flex shrink-0 overflow-hidden rounded border border-border" :title="canContinue ? '' : reason">
    <button
      type="button"
      class="min-h-7 px-2.5 py-1 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-45"
      :disabled="!canContinue || continuing || menuOpening"
      @click="continueInMain()"
    >
      <span class="i-carbon-arrow-right mr-1 h-3 w-3" aria-hidden="true" />
      {{ continuing ? t('worktreeSession.continuing') : t('worktreeSession.continueInMain') }}
    </button>
    <button
      v-if="ordinaryTabs.length"
      type="button"
      class="flex min-h-7 w-7 items-center justify-center border-l border-border transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-45"
      :disabled="!canContinue || continuing || menuOpening"
      :title="t('workbench.chooseTarget')"
      :aria-label="t('workbench.chooseTarget')"
      @click="chooseTarget"
    >
      <span class="i-carbon-chevron-down h-3 w-3" aria-hidden="true" />
    </button>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { relaunch } from '@tauri-apps/plugin-process'
import { useEngines } from '@/engines/useEngines'
import { instanceKey } from '@/engines/identity'
import { setEngineEnabled } from '@/engines/client'
import { useConfirm } from '@/composables/useConfirm'
import type { EngineDescriptor } from '@/engines/types'
import ClaudeEnvCard from './ClaudeEnvCard.vue'
import ClaudeDataDirCard from './ClaudeDataDirCard.vue'
import CodexEnvCard from './CodexEnvCard.vue'

const { t } = useI18n()
const { confirm } = useConfirm()
const { engines, health, loading, errors, refreshEngines } = useEngines()
const expanded = ref<Set<string>>(new Set())
const exporting = ref(false)
const exportStatus = ref<string | null>(null)
const updating = ref<Set<string>>(new Set())

async function exportDiagnostics() {
  const path = await save({ defaultPath: 'monet-engine-diagnostics.json', filters: [{ name: 'JSON', extensions: ['json'] }] })
  if (!path) return
  exporting.value = true
  exportStatus.value = null
  try {
    await invoke('engine_export_diagnostics', { path })
    exportStatus.value = t('engineSettings.exported')
  } catch (cause) {
    exportStatus.value = String(cause)
  } finally {
    exporting.value = false
  }
}

function toggle(key: string) {
  const next = new Set(expanded.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  expanded.value = next
}

function statusClass(status?: string) {
  if (status === 'available') return 'bg-primary'
  if (status === 'degraded') return 'bg-accent'
  if (status === 'disabled') return 'bg-muted-foreground'
  return 'bg-destructive'
}

function capabilityStatus(available?: boolean, reasonCode?: string | null) {
  if (available) return t('engineSettings.available')
  return reasonCode ? t(reasonCode, t('engineSettings.unavailable')) : t('engineSettings.unavailable')
}

async function toggleEnabled(engine: EngineDescriptor) {
  const key = instanceKey(engine.instance)
  updating.value = new Set(updating.value).add(key)
  exportStatus.value = null
  try {
    await setEngineEnabled(engine.instance, !engine.enabled)
    await refreshEngines()
    const shouldRestart = await confirm(t('engineSettings.restartConfirm'), t('engineSettings.restart'))
    if (shouldRestart) {
      exportStatus.value = t('engineSettings.restarting')
      await relaunch()
    } else {
      exportStatus.value = t('engineSettings.restartLater')
    }
  } catch (cause) {
    exportStatus.value = String(cause)
  } finally {
    const next = new Set(updating.value)
    next.delete(key)
    updating.value = next
  }
}

async function openGuide(url: string | null) {
  if (!url) return
  try {
    await invoke('open_in_default_app', { path: url })
  } catch (cause) {
    exportStatus.value = String(cause)
  }
}

async function openConfiguration(engine: EngineDescriptor) {
  try {
    await invoke('engine_open_configuration', { instance: engine.instance })
  } catch (cause) {
    exportStatus.value = String(cause)
  }
}
</script>

<template>
  <section>
    <div class="mb-3 flex items-start justify-between gap-4">
      <div>
        <h2 class="text-[13px] font-semibold">{{ t('engineSettings.title') }}</h2>
        <p class="mt-1 text-xs leading-relaxed text-muted-foreground">{{ t('engineSettings.description') }}</p>
      </div>
      <div class="flex shrink-0 items-center gap-1.5">
        <button type="button" class="rounded border border-border px-2.5 py-1.5 text-xs hover:bg-muted disabled:opacity-50" :disabled="exporting" @click="exportDiagnostics">
          <span class="i-carbon-download mr-1 inline-block h-3 w-3 align-text-bottom" />{{ t('engineSettings.exportDiagnostics') }}
        </button>
        <button type="button" class="rounded border border-border px-2.5 py-1.5 text-xs hover:bg-muted disabled:opacity-50" :disabled="loading" @click="refreshEngines">
          <span class="mr-1 inline-block h-3 w-3 align-text-bottom" :class="loading ? 'i-carbon-renew animate-spin' : 'i-carbon-renew'" />
          {{ t('common.refresh') }}
        </button>
      </div>
    </div>
    <p v-if="exportStatus" role="status" class="mb-2 text-[10px] text-muted-foreground">{{ exportStatus }}</p>

    <div class="grid grid-cols-[repeat(auto-fit,minmax(360px,1fr))] items-start gap-3">
      <article v-for="engine in engines" :key="instanceKey(engine.instance)" class="overflow-hidden rounded border border-border bg-card shadow-paper">
        <button type="button" class="flex w-full items-center gap-2 px-3 py-2.5 text-left hover:bg-muted/60" :aria-expanded="expanded.has(instanceKey(engine.instance))" @click="toggle(instanceKey(engine.instance))">
          <span class="h-2 w-2 shrink-0 rounded-full" :class="statusClass(health[instanceKey(engine.instance)]?.status)" />
          <span class="min-w-0 flex-1">
            <span class="block truncate text-xs font-semibold">{{ engine.displayName }}</span>
            <span class="block truncate text-[10px] text-muted-foreground">{{ engine.instance.engineId }} / {{ engine.instance.instanceId }}</span>
          </span>
          <span class="text-[10px] text-muted-foreground">{{ t(`engineSettings.status.${health[instanceKey(engine.instance)]?.status ?? 'unavailable'}`) }}</span>
          <span class="h-3.5 w-3.5 text-muted-foreground" :class="expanded.has(instanceKey(engine.instance)) ? 'i-carbon-chevron-up' : 'i-carbon-chevron-down'" />
        </button>

        <div v-if="expanded.has(instanceKey(engine.instance))" class="border-t border-border px-3 py-2.5">
          <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-[11px]">
            <dt class="text-muted-foreground">{{ t('engineSettings.activation') }}</dt>
            <dd>{{ engine.enabled ? t('common.enabled') : t('engineSettings.disabled') }}</dd>
            <dt class="text-muted-foreground">{{ t('engineSettings.installed') }}</dt>
            <dd>{{ engine.enabled ? (health[instanceKey(engine.instance)]?.installed ? t('engineSettings.installedStatus') : t('engineSettings.notInstalled')) : '—' }}</dd>
            <dt class="text-muted-foreground">{{ t('engineSettings.version') }}</dt>
            <dd>{{ health[instanceKey(engine.instance)]?.version || '—' }}</dd>
            <dt class="text-muted-foreground">{{ t('engineSettings.authentication') }}</dt>
            <dd>{{ health[instanceKey(engine.instance)]?.authenticated == null ? '—' : (health[instanceKey(engine.instance)]?.authenticated ? t('engineSettings.authenticated') : t('engineSettings.notAuthenticated')) }}</dd>
            <dt class="text-muted-foreground">{{ t('engineSettings.path') }}</dt>
            <dd class="truncate font-mono" :title="health[instanceKey(engine.instance)]?.executablePath || ''">{{ health[instanceKey(engine.instance)]?.executablePath || '—' }}</dd>
            <dt class="text-muted-foreground">{{ t('engineSettings.source') }}</dt>
            <dd>{{ capabilityStatus(health[instanceKey(engine.instance)]?.source.available, health[instanceKey(engine.instance)]?.source.reasonCode) }}</dd>
            <dt class="text-muted-foreground">{{ t('engineSettings.runtime') }}</dt>
            <dd>{{ capabilityStatus(health[instanceKey(engine.instance)]?.runtime.available, health[instanceKey(engine.instance)]?.runtime.reasonCode) }}</dd>
          </dl>

          <div class="mt-2 flex flex-wrap gap-1">
            <span v-if="engine.capabilities.history.search" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.search') }}</span>
            <span v-if="engine.capabilities.history.assets" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.assets') }}</span>
            <span v-if="engine.capabilities.runtime?.resume" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.runtime') }}</span>
            <span v-if="engine.capabilities.runtime?.steer" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.steer') }}</span>
            <span v-if="engine.capabilities.facets.quota" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.quota') }}</span>
            <span v-if="engine.capabilities.facets.configuration" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.configuration') }}</span>
            <span v-if="engine.capabilities.facets.runtimeCommands" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.runtimeCommands') }}</span>
            <span v-if="engine.capabilities.facets.automation" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.automation') }}</span>
          </div>

          <div v-if="engine.instance.engineId === 'claude-code' || engine.instance.engineId === 'codex'" class="mt-3 border-t border-border pt-2">
            <p class="mb-2 text-[10px] font-medium text-muted-foreground">{{ t('engineSettings.engineCapabilities') }}</p>
            <template v-if="engine.instance.engineId === 'claude-code'">
              <ClaudeEnvCard />
              <ClaudeDataDirCard />
            </template>
            <CodexEnvCard v-else />
          </div>

          <div v-if="health[instanceKey(engine.instance)]?.diagnostics.length || errors[instanceKey(engine.instance)]" role="status" class="mt-2 rounded border border-destructive/30 bg-destructive/5 p-2 text-[10px] text-destructive">
            <p v-if="errors[instanceKey(engine.instance)]">{{ errors[instanceKey(engine.instance)] }}</p>
            <p v-for="diagnostic in health[instanceKey(engine.instance)]?.diagnostics" :key="diagnostic.code">{{ diagnostic.message }}</p>
          </div>
          <div class="mt-2 flex flex-wrap justify-end gap-1.5 border-t border-border pt-2">
            <button v-if="engine.ui.installGuideUrl" type="button" class="rounded border border-border px-2.5 py-1 text-[11px] hover:bg-muted" @click="openGuide(engine.ui.installGuideUrl)">
              {{ t('engineSettings.installGuide') }}
            </button>
            <button v-if="engine.enabled && engine.capabilities.facets.configuration" type="button" class="rounded border border-border px-2.5 py-1 text-[11px] hover:bg-muted" @click="openConfiguration(engine)">
              <span class="i-carbon-edit mr-1 inline-block h-3 w-3 align-text-bottom" />{{ t('engineSettings.editConfiguration') }}
            </button>
            <button type="button" class="rounded border border-border px-2.5 py-1 text-[11px] hover:bg-muted disabled:opacity-50" :disabled="updating.has(instanceKey(engine.instance))" @click="toggleEnabled(engine)">
              {{ engine.enabled ? t('engineSettings.ignoreEngine') : t('engineSettings.restoreEngine') }}
            </button>
          </div>
        </div>
      </article>
    </div>
  </section>
</template>

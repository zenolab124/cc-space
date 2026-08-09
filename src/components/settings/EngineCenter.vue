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
  <section class="settings-page settings-page-engine">
    <header class="settings-page-hero">
      <div class="settings-page-hero-copy">
        <div class="settings-page-eyebrow">{{ t('settings.settingsKicker') }}</div>
        <h2 class="settings-page-title">{{ t('engineSettings.title') }}</h2>
        <p class="settings-page-intro">{{ t('engineSettings.description') }}</p>
      </div>
      <div class="settings-page-hero-actions">
        <button type="button" class="settings-page-button" :disabled="exporting" @click="exportDiagnostics">
          <span class="i-carbon-download mr-1 inline-block h-3 w-3 align-text-bottom" />{{ t('engineSettings.exportDiagnostics') }}
        </button>
        <button type="button" class="settings-page-button" :disabled="loading" @click="refreshEngines">
          <span class="mr-1 inline-block h-3 w-3 align-text-bottom" :class="loading ? 'i-carbon-renew animate-spin' : 'i-carbon-renew'" />
          {{ t('common.refresh') }}
        </button>
      </div>
    </header>
    <p v-if="exportStatus" role="status" class="mb-2 text-[10px] text-muted-foreground">{{ exportStatus }}</p>

    <div class="settings-engine-grid">
      <article v-for="engine in engines" :key="instanceKey(engine.instance)" class="settings-engine-card">
        <button type="button" class="settings-engine-card-toggle" :aria-expanded="expanded.has(instanceKey(engine.instance))" @click="toggle(instanceKey(engine.instance))">
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
            <span v-if="engine.capabilities.runtime?.sendWhileRunning" class="rounded bg-secondary px-1.5 py-0.5 text-[10px]">{{ t('engineSettings.capability.sendWhileRunning') }}</span>
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

<style scoped>
.settings-page {
  padding: 2px 0 28px;
}
.settings-page-hero {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 24px;
  padding: 4px 2px 20px;
  border-bottom: 1px solid var(--border);
}
.settings-page-hero-copy { min-width: 0; }
.settings-page-eyebrow {
  margin-bottom: 5px;
  color: var(--primary);
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.14em;
}
.settings-page-title {
  margin: 0;
  font-size: 24px;
  line-height: 1.2;
  letter-spacing: -0.02em;
}
.settings-page-intro {
  max-width: 620px;
  margin: 7px 0 0;
  color: var(--muted-foreground);
  font-size: 12px;
  line-height: 1.7;
}
.settings-page-hero-actions {
  display: flex;
  flex-shrink: 0;
  gap: 6px;
}
.settings-page-button {
  display: inline-flex;
  align-items: center;
  padding: 7px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  color: var(--muted-foreground);
  background: var(--card);
  font-size: 11px;
  cursor: pointer;
}
.settings-page-button:hover:not(:disabled) { color: var(--foreground); background: var(--muted); }
.settings-page-button:disabled { opacity: 0.5; cursor: not-allowed; }
.settings-engine-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
  align-items: start;
  gap: 14px;
  margin-top: 16px;
}
.settings-engine-card {
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
  box-shadow: var(--shadow-paper);
}
.settings-engine-card-toggle {
  display: flex;
  align-items: center;
  width: 100%;
  gap: 9px;
  padding: 13px 16px;
  color: var(--foreground);
  background: transparent;
  text-align: left;
  cursor: pointer;
}
.settings-engine-card-toggle:hover { background: color-mix(in srgb, var(--primary) 4%, transparent); }
.settings-engine-card > div {
  padding: 14px 16px;
}
@media (max-width: 680px) {
  .settings-page-hero { align-items: flex-start; flex-direction: column; }
  .settings-page-hero-actions { width: 100%; }
  .settings-page-button { flex: 1; justify-content: center; }
  .settings-engine-grid { grid-template-columns: minmax(0, 1fr); }
}
</style>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  useChannels,
  type ChannelInfo,
  type ProbeResult,
  APPLE_FM_CHANNEL_ID,
} from '@/composables/useChannels'
import { adapterEndpointUrl, preferredAdapterBaseUrl } from '@/utils/channelUrls'
import ChannelModelMap from './ChannelModelMap.vue'

const props = defineProps<{ channel: ChannelInfo | null }>()
const emit = defineEmits<{ (e: 'saved'): void; (e: 'cancel'): void }>()

const { t } = useI18n()
const { saveChannel, revealToken, probing, probeChannel } = useChannels()
const isNew = computed(() => props.channel === null)
const isVirtual = computed(() => props.channel?.id === APPLE_FM_CHANNEL_ID)

const id = ref(props.channel?.id ?? '')
const name = ref(props.channel?.name ?? '')
const baseUrl = ref(props.channel?.baseUrl ?? '')
const authMode = ref<'bearer' | 'none'>(props.channel?.authMode === 'none' ? 'none' : 'bearer')
const authToken = ref('')
const tokenVisible = ref(false)
const note = ref(props.channel?.note ?? '')
const modelsText = ref((props.channel?.availableModels ?? []).join('\n'))
const engineSupport = ref<string[]>(props.channel
  ? props.channel.scope === 'agent-only' ? [] : [...props.channel.engineSupport]
  : ['claude-code'])

const claudeBaseUrl = ref(props.channel?.claude?.baseUrl ?? '')
const claudeAuthMode = ref<'inherit' | 'bearer' | 'none'>(props.channel?.claude?.authMode ?? 'inherit')
const claudeAuthToken = ref('')
const claudeTokenVisible = ref(false)
const claudeProbe = ref<ProbeResult | null>(null)

const codexProviderId = ref(props.channel?.codex?.providerId ?? `monet-${id.value || 'proxy'}`)
const codexManagedProviderIdAuto = ref(!props.channel?.codex?.providerId)
const codexMode = ref<'external' | 'managed'>(props.channel?.codex?.mode === 'external' ? 'external' : 'managed')
const codexBaseUrl = ref(props.channel?.codex?.baseUrl ?? '')
const codexAuthMode = ref<'inherit' | 'bearer' | 'openai' | 'none'>(
  props.channel?.codex?.authMode === 'bearer'
    || props.channel?.codex?.authMode === 'openai'
    || props.channel?.codex?.authMode === 'none'
    ? props.channel.codex.authMode
    : 'inherit',
)
const codexAuthToken = ref('')
const codexTokenVisible = ref(false)
const codexProbe = ref<ProbeResult | null>(null)

const sourceModelEnv = computed<Record<string, string>>(() => props.channel?.modelEnv ?? {})
const modelEnv = ref<Record<string, string>>({ ...(props.channel?.modelEnv ?? {}) })
const parsedModels = computed(() => [...new Set(modelsText.value
  .split(/[,，\n]/)
  .map(model => model.trim())
  .filter(Boolean))].sort())
const modelOptions = computed(() => parsedModels.value)

const supportsClaude = computed(() => engineSupport.value.includes('claude-code'))
const supportsCodex = computed(() => engineSupport.value.includes('codex'))
const isLegacyAgentOnly = computed(() => !isNew.value && !isVirtual.value && props.channel?.scope === 'agent-only')
const legacyCompatibilityMode = computed(() => isLegacyAgentOnly.value && engineSupport.value.length === 0)
const usesSharedConnection = computed(() => legacyCompatibilityMode.value
  || supportsClaude.value
  || (supportsCodex.value && codexMode.value === 'managed'))
const probeTargetId = computed(() => props.channel?.id ?? id.value)
const modelMapProbing = computed(() => !!probing.value[probeTargetId.value])
const managedProviderIdPlaceholder = computed(() => `monet-${id.value.trim() || 'proxy'}`)

const claudeSourceUrl = computed(() => claudeBaseUrl.value.trim() || baseUrl.value.trim())
const codexSourceUrl = computed(() => codexBaseUrl.value.trim() || baseUrl.value.trim())
const initialClaudeSourceUrl = props.channel?.claude?.baseUrl?.trim() || props.channel?.baseUrl?.trim() || ''
const initialCodexSourceUrl = props.channel?.codex?.baseUrl?.trim() || props.channel?.baseUrl?.trim() || ''
const sameSourceUrl = (left: string, right: string) => left.replace(/\/+$/, '') === right.replace(/\/+$/, '')
const claudeResolvedCache = computed(() => claudeProbe.value?.resolvedBaseUrl
  ?? (sameSourceUrl(claudeSourceUrl.value, initialClaudeSourceUrl) ? props.channel?.claude?.cachedBaseUrl : undefined))
const codexResolvedCache = computed(() => codexProbe.value?.resolvedBaseUrl
  ?? (sameSourceUrl(codexSourceUrl.value, initialCodexSourceUrl) ? props.channel?.codex?.cachedBaseUrl : undefined))
const claudeResolvedBase = computed(() => claudeProbe.value?.resolvedBaseUrl
  ?? (sameSourceUrl(claudeSourceUrl.value, initialClaudeSourceUrl) ? props.channel?.claude?.resolvedBaseUrl : undefined)
  ?? preferredAdapterBaseUrl(claudeSourceUrl.value, 'claude-code'))
const codexResolvedBase = computed(() => codexProbe.value?.resolvedBaseUrl
  ?? (sameSourceUrl(codexSourceUrl.value, initialCodexSourceUrl) ? props.channel?.codex?.resolvedBaseUrl : undefined)
  ?? preferredAdapterBaseUrl(codexSourceUrl.value, 'codex'))
const claudeEndpoint = computed(() => claudeResolvedBase.value
  ? adapterEndpointUrl(claudeResolvedBase.value, 'claude-code') : '')
const codexEndpoint = computed(() => codexResolvedBase.value
  ? adapterEndpointUrl(codexResolvedBase.value, 'codex') : '')

const saving = ref(false)
const formError = ref<string | null>(null)
const ID_PATTERN = /^[a-zA-Z0-9_-]{1,64}$/

function onModelEnvUpdate(env: Record<string, string>) {
  modelEnv.value = env
}

function toggleEngine(engineId: string) {
  if (engineSupport.value.includes(engineId)) {
    engineSupport.value = engineSupport.value.filter(engine => engine !== engineId)
  } else {
    engineSupport.value = [...engineSupport.value, engineId]
  }
}

function onCodexManagedProviderIdInput() {
  codexManagedProviderIdAuto.value = false
}

function effectiveToken(adapter: 'claude-code' | 'codex'): string {
  if (adapter === 'claude-code') {
    if (claudeAuthMode.value === 'none') return ''
    return claudeAuthToken.value.trim() || (authMode.value === 'bearer' ? authToken.value.trim() : '')
  }
  if (codexAuthMode.value === 'none' || codexAuthMode.value === 'openai') return ''
  return codexAuthToken.value.trim() || (authMode.value === 'bearer' ? authToken.value.trim() : '')
}

function mergeProbeModels(result: ProbeResult | null) {
  if (!result?.models.length) return
  modelsText.value = [...new Set([...parsedModels.value, ...result.models])].sort().join('\n')
}

async function probeAdapter(adapter: 'claude-code' | 'codex'): Promise<ProbeResult | null> {
  const target = probeTargetId.value.trim()
  const sourceUrl = adapter === 'claude-code' ? claudeSourceUrl.value : codexSourceUrl.value
  if (!target) {
    formError.value = t('settings.channelForm.idError')
    return null
  }
  if (!sourceUrl) {
    formError.value = t('settings.channelForm.baseUrlError')
    return null
  }
  formError.value = null
  const result = await probeChannel(target, {
    baseUrl: sourceUrl,
    token: effectiveToken(adapter),
    adapter,
  })
  if (adapter === 'claude-code') claudeProbe.value = result
  else codexProbe.value = result
  mergeProbeModels(result)
  return result
}

watch(id, () => {
  if (codexManagedProviderIdAuto.value) codexProviderId.value = managedProviderIdPlaceholder.value
})
watch([baseUrl, authMode, authToken, claudeBaseUrl, claudeAuthMode, claudeAuthToken], () => { claudeProbe.value = null })
watch([baseUrl, authMode, authToken, codexMode, codexBaseUrl, codexAuthMode, codexAuthToken], () => { codexProbe.value = null })

onMounted(async () => {
  if (isNew.value || !props.channel) return
  const loads: Promise<void>[] = [
    revealToken(props.channel.id, 'shared').then(token => { if (token) authToken.value = token }),
  ]
  if (props.channel.claude?.authTokenMasked) {
    loads.push(revealToken(props.channel.id, 'claude-code').then(token => { if (token) claudeAuthToken.value = token }))
  }
  if (props.channel.codex?.authTokenMasked) {
    loads.push(revealToken(props.channel.id, 'codex').then(token => { if (token) codexAuthToken.value = token }))
  }
  await Promise.all(loads)
})

async function onSave() {
  formError.value = null
  const trimmedId = id.value.trim()
  if (!ID_PATTERN.test(trimmedId) || trimmedId === 'official') {
    formError.value = t('settings.channelForm.idError')
    return
  }
  if (!name.value.trim()) {
    formError.value = t('settings.channelForm.nameError')
    return
  }
  if (!engineSupport.value.length && !legacyCompatibilityMode.value) {
    formError.value = t('settings.channelForm.engineRequired')
    return
  }
  if (!isVirtual.value && usesSharedConnection.value && !baseUrl.value.trim()) {
    formError.value = t('settings.channelForm.baseUrlError')
    return
  }
  if (!isVirtual.value && usesSharedConnection.value && authMode.value === 'bearer' && !authToken.value.trim()) {
    formError.value = t('settings.channelForm.tokenError')
    return
  }
  if (supportsClaude.value && claudeAuthMode.value === 'bearer' && !effectiveToken('claude-code')) {
    formError.value = t('settings.channelForm.tokenError')
    return
  }
  if (supportsCodex.value && codexMode.value === 'managed' && codexAuthMode.value === 'bearer' && !effectiveToken('codex')) {
    formError.value = t('settings.channelForm.tokenError')
    return
  }
  if (supportsCodex.value && !codexProviderId.value.trim()) {
    formError.value = t('settings.channelForm.codexProviderError')
    return
  }

  saving.value = true
  try {
    const probeTasks: Promise<ProbeResult | null>[] = []
    if (supportsClaude.value && !claudeProbe.value) probeTasks.push(probeAdapter('claude-code'))
    if (supportsCodex.value && codexMode.value === 'managed' && !codexProbe.value) probeTasks.push(probeAdapter('codex'))
    await Promise.all(probeTasks)

    await saveChannel({
      id: trimmedId,
      name: name.value.trim(),
      baseUrl: isVirtual.value || !usesSharedConnection.value ? '' : baseUrl.value.trim().replace(/\/+$/, ''),
      authMode: usesSharedConnection.value ? authMode.value : 'none',
      authToken: usesSharedConnection.value ? authToken.value.trim() : '',
      note: note.value.trim() || undefined,
      protocol: legacyCompatibilityMode.value ? props.channel?.protocol : undefined,
      scope: legacyCompatibilityMode.value ? 'agent-only' : undefined,
      availableModels: parsedModels.value,
      modelEnv: isVirtual.value ? undefined : modelEnv.value,
      engineSupport: [...engineSupport.value],
      claude: supportsClaude.value ? {
        baseUrl: claudeBaseUrl.value.trim().replace(/\/+$/, '') || undefined,
        authMode: claudeAuthMode.value,
        authToken: claudeAuthToken.value.trim(),
        resolvedBaseUrl: claudeResolvedCache.value || undefined,
      } : undefined,
      codex: supportsCodex.value ? {
        mode: codexMode.value,
        providerId: codexProviderId.value.trim(),
        baseUrl: codexMode.value === 'managed' ? codexBaseUrl.value.trim().replace(/\/+$/, '') || undefined : undefined,
        authMode: codexMode.value === 'managed' ? codexAuthMode.value : undefined,
        authToken: codexMode.value === 'managed' ? codexAuthToken.value.trim() : undefined,
        resolvedBaseUrl: codexMode.value === 'managed' ? codexResolvedCache.value || undefined : undefined,
      } : undefined,
    })
    emit('saved')
  } catch (error) {
    formError.value = String(error)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="rounded-md border border-border bg-popover p-3 flex flex-col gap-2.5">
    <div class="text-xs font-medium">{{ isNew ? $t('settings.channelForm.newTitle') : $t('settings.channelForm.editTitle', { id: channel!.id }) }}</div>

    <label class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.idLabel') }}</span>
      <input v-model="id" :disabled="!isNew" type="text" :placeholder="$t('settings.channelForm.idPlaceholder')" class="form-input disabled:opacity-50" spellcheck="false" />
    </label>
    <label class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.nameLabel') }}</span>
      <input v-model="name" type="text" :placeholder="$t('settings.channelForm.namePlaceholder')" class="form-input" />
    </label>

    <section v-if="!isVirtual && usesSharedConnection" class="engine-binding connection-binding">
      <div class="engine-binding-head">
        <span class="i-carbon-link h-3.5 w-3.5 text-primary" />
        <span>{{ $t('settings.channelForm.connectionTitle') }}</span>
        <span class="ml-auto text-[9px] font-normal text-muted-foreground">{{ $t('settings.channelForm.connectionShared') }}</span>
      </div>
      <label class="form-field">
        <span class="form-label">Base URL <span class="text-muted-foreground font-normal">{{ $t('settings.channelForm.baseUrlHint') }}</span></span>
        <input v-model="baseUrl" type="url" placeholder="https://api.example.com" class="form-input font-mono" spellcheck="false" />
      </label>
      <label class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.authModeLabel') }}</span>
        <select v-model="authMode" class="form-input">
          <option value="bearer">API Key / Bearer Token</option>
          <option value="none">{{ $t('settings.channelForm.codexAuthNone') }}</option>
        </select>
      </label>
      <div v-if="authMode === 'bearer'" class="form-field">
        <span class="form-label">API Key</span>
        <div class="relative">
          <input v-model="authToken" :type="tokenVisible ? 'text' : 'password'" placeholder="sk-…" class="form-input font-mono w-full pr-8" autocomplete="off" />
          <button type="button" class="token-visibility" :aria-label="$t('settings.channelForm.toggleTokenVisibility')" @click="tokenVisible = !tokenVisible">
            <span :class="tokenVisible ? 'i-carbon-view-off' : 'i-carbon-view'" class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
      <label class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.modelsLabel') }}</span>
        <textarea v-model="modelsText" rows="3" class="form-input resize-y font-mono" placeholder="claude-sonnet-4-5&#10;gpt-5.2" spellcheck="false" />
        <span class="form-help">{{ $t('settings.channelForm.modelsHint') }}</span>
      </label>
    </section>

    <div class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.engineSupportLabel') }}</span>
      <div class="flex items-center gap-1.5">
        <button v-for="engine in [{ id: 'claude-code', label: 'Claude Code' }, { id: 'codex', label: 'Codex' }]" :key="engine.id" type="button" class="engine-chip" :class="{ active: engineSupport.includes(engine.id) }" :aria-pressed="engineSupport.includes(engine.id)" @click="toggleEngine(engine.id)">
          <span class="h-1.5 w-1.5 rounded-full" :class="engine.id === 'codex' ? 'bg-codex' : 'bg-claude'" />{{ engine.label }}
        </button>
      </div>
      <span class="form-help">{{ $t('settings.channelForm.engineSupportHint') }}</span>
    </div>

    <section v-if="supportsClaude" class="engine-binding">
      <div class="engine-binding-head">
        <span class="h-1.5 w-1.5 rounded-full bg-claude" /><span>Claude Code</span>
        <span class="protocol-badge font-mono">Anthropic Messages</span>
      </div>
      <div class="endpoint-preview">
        <span>{{ $t('settings.channelForm.actualEndpoint') }}</span>
        <code>{{ claudeEndpoint || '—' }}</code>
        <button type="button" :disabled="modelMapProbing" @click="probeAdapter('claude-code')">{{ $t('settings.channelForm.testAdapter') }}</button>
      </div>
      <p v-if="claudeProbe" class="probe-status" :class="claudeProbe.online ? 'text-green-600' : 'text-destructive'">
        {{ $t(claudeProbe.online ? 'settings.channelForm.probeReady' : 'settings.channelForm.probeFailed', { status: claudeProbe.status }) }}
      </p>
      <ChannelModelMap :model-env="sourceModelEnv" :model-options="modelOptions" :probing="modelMapProbing" :dom-key="id || 'new'" @update:env="onModelEnvUpdate" @probe="probeAdapter('claude-code')" />
      <details class="adapter-advanced">
        <summary>{{ $t('settings.channelForm.advancedOverride') }}</summary>
        <p class="form-help">{{ $t('settings.channelForm.advancedOverrideHint') }}</p>
        <label class="form-field">
          <span class="form-label">Base URL</span>
          <input v-model="claudeBaseUrl" type="url" :placeholder="$t('settings.channelForm.inheritShared')" class="form-input font-mono" spellcheck="false" />
        </label>
        <label class="form-field">
          <span class="form-label">{{ $t('settings.channelForm.authModeLabel') }}</span>
          <select v-model="claudeAuthMode" class="form-input">
            <option value="inherit">{{ $t('settings.channelForm.inheritShared') }}</option>
            <option value="bearer">API Key / Bearer Token</option>
            <option value="none">{{ $t('settings.channelForm.codexAuthNone') }}</option>
          </select>
        </label>
        <div v-if="claudeAuthMode !== 'none'" class="form-field">
          <span class="form-label">API Key <span class="text-muted-foreground font-normal">{{ $t('settings.channelForm.blankInherits') }}</span></span>
          <div class="relative">
            <input v-model="claudeAuthToken" :type="claudeTokenVisible ? 'text' : 'password'" class="form-input font-mono w-full pr-8" autocomplete="off" />
            <button type="button" class="token-visibility" :aria-label="$t('settings.channelForm.toggleTokenVisibility')" @click="claudeTokenVisible = !claudeTokenVisible"><span :class="claudeTokenVisible ? 'i-carbon-view-off' : 'i-carbon-view'" class="w-3.5 h-3.5" /></button>
          </div>
        </div>
      </details>
    </section>

    <section v-if="supportsCodex" class="engine-binding">
      <div class="engine-binding-head">
        <span class="h-1.5 w-1.5 rounded-full bg-codex" /><span>Codex</span>
        <span class="protocol-badge font-mono">OpenAI Responses</span>
      </div>
      <label class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.codexModeLabel') }}</span>
        <select v-model="codexMode" class="form-input">
          <option value="managed">{{ $t('settings.channelForm.codexModeManaged') }}</option>
          <option v-if="channel?.codex?.mode === 'external'" value="external">{{ $t('settings.channelForm.codexModeExternal') }}</option>
        </select>
        <span class="form-help">{{ $t(codexMode === 'external' ? 'settings.channelForm.codexModeExternalHint' : 'settings.channelForm.codexModeManagedHint') }}</span>
      </label>
      <label class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.codexProviderLabel') }}</span>
        <input v-model="codexProviderId" type="text" :placeholder="managedProviderIdPlaceholder" class="form-input font-mono" spellcheck="false" @input="onCodexManagedProviderIdInput" />
        <span class="form-help">{{ $t('settings.channelForm.codexAdapterHint') }}</span>
      </label>
      <div v-if="codexMode === 'managed'" class="endpoint-preview">
        <span>{{ $t('settings.channelForm.actualEndpoint') }}</span>
        <code>{{ codexEndpoint || '—' }}</code>
        <button type="button" :disabled="modelMapProbing" @click="probeAdapter('codex')">{{ $t('settings.channelForm.testAdapter') }}</button>
      </div>
      <p v-if="codexMode === 'managed' && codexProbe" class="probe-status" :class="codexProbe.online ? 'text-green-600' : 'text-destructive'">
        {{ $t(codexProbe.online ? 'settings.channelForm.probeReady' : 'settings.channelForm.probeFailed', { status: codexProbe.status }) }}
      </p>
      <details v-if="codexMode === 'managed'" class="adapter-advanced">
        <summary>{{ $t('settings.channelForm.advancedOverride') }}</summary>
        <p class="form-help">{{ $t('settings.channelForm.advancedOverrideHint') }}</p>
        <label class="form-field">
          <span class="form-label">Base URL</span>
          <input v-model="codexBaseUrl" type="url" :placeholder="$t('settings.channelForm.inheritShared')" class="form-input font-mono" spellcheck="false" />
        </label>
        <label class="form-field">
          <span class="form-label">{{ $t('settings.channelForm.authModeLabel') }}</span>
          <select v-model="codexAuthMode" class="form-input">
            <option value="inherit">{{ $t('settings.channelForm.inheritShared') }}</option>
            <option value="bearer">Bearer Token</option>
            <option value="openai">{{ $t('settings.channelForm.codexAuthOpenai') }}</option>
            <option value="none">{{ $t('settings.channelForm.codexAuthNone') }}</option>
          </select>
        </label>
        <div v-if="codexAuthMode === 'inherit' || codexAuthMode === 'bearer'" class="form-field">
          <span class="form-label">API Key <span class="text-muted-foreground font-normal">{{ $t('settings.channelForm.blankInherits') }}</span></span>
          <div class="relative">
            <input v-model="codexAuthToken" :type="codexTokenVisible ? 'text' : 'password'" class="form-input font-mono w-full pr-8" autocomplete="off" />
            <button type="button" class="token-visibility" :aria-label="$t('settings.channelForm.toggleTokenVisibility')" @click="codexTokenVisible = !codexTokenVisible"><span :class="codexTokenVisible ? 'i-carbon-view-off' : 'i-carbon-view'" class="w-3.5 h-3.5" /></button>
          </div>
        </div>
      </details>
    </section>

    <label class="form-field"><span class="form-label">{{ $t('settings.channelForm.noteLabel') }}</span><input v-model="note" type="text" :placeholder="$t('common.optional')" class="form-input" /></label>
    <p v-if="formError" class="text-xs text-destructive" role="alert">{{ formError }}</p>
    <div class="flex items-center gap-2 justify-end">
      <button class="px-2.5 py-1 text-xs rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors" @click="emit('cancel')">{{ $t('common.cancel') }}</button>
      <button :disabled="saving" class="px-2.5 py-1 text-xs rounded-md bg-primary text-primary-foreground hover:shadow-paper transition-shadow disabled:opacity-50" @click="onSave">{{ saving ? $t('common.saving') : $t('common.save') }}</button>
    </div>
  </div>
</template>

<style scoped>
.form-field { display: flex; flex-direction: column; gap: 4px; }
.form-label { font-size: 11px; color: var(--muted-foreground); }
.form-help { font-size: 10px; line-height: 1.45; color: color-mix(in srgb, var(--muted-foreground) 75%, transparent); }
.engine-chip { display: inline-flex; align-items: center; gap: 5px; min-height: 28px; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius); color: var(--muted-foreground); font-size: 11px; transition: color .15s, border-color .15s, background .15s; cursor: pointer; }
.engine-chip:hover { color: var(--foreground); background: var(--muted); }
.engine-chip.active { color: var(--foreground); border-color: color-mix(in srgb, var(--primary) 55%, var(--border)); background: color-mix(in srgb, var(--primary) 8%, transparent); }
.engine-binding { display: flex; flex-direction: column; gap: 10px; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); }
.engine-binding-head { display: flex; align-items: center; gap: 6px; font-size: 11px; font-weight: 600; }
.connection-binding { border-color: color-mix(in srgb, var(--primary) 30%, var(--border)); background: color-mix(in srgb, var(--primary) 3%, var(--card)); }
.protocol-badge { margin-left: auto; font-size: 9px; font-weight: 400; color: var(--muted-foreground); }
.endpoint-preview { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 6px; padding: 6px 8px; border: 1px solid color-mix(in srgb, var(--border) 70%, transparent); border-radius: var(--radius); background: color-mix(in srgb, var(--muted) 45%, transparent); font-size: 10px; color: var(--muted-foreground); }
.endpoint-preview code { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--foreground); }
.endpoint-preview button { color: var(--primary); cursor: pointer; }
.endpoint-preview button:disabled { opacity: .45; cursor: default; }
.probe-status { margin: -4px 0 0; font-size: 10px; }
.adapter-advanced { padding-top: 2px; border-top: 1px solid color-mix(in srgb, var(--border) 60%, transparent); }
.adapter-advanced summary { padding-top: 7px; font-size: 10px; color: var(--muted-foreground); cursor: pointer; }
.adapter-advanced[open] { display: flex; flex-direction: column; gap: 8px; }
.token-visibility { position: absolute; right: 6px; top: 50%; display: inline-flex; width: 28px; height: 28px; align-items: center; justify-content: center; transform: translateY(-50%); color: var(--muted-foreground); cursor: pointer; transition: color .15s; }
.token-visibility:hover { color: var(--foreground); }
</style>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChannels, type ChannelInfo, APPLE_FM_CHANNEL_ID } from '@/composables/useChannels'
import ChannelModelMap from './ChannelModelMap.vue'

const props = defineProps<{
  channel: ChannelInfo | null
}>()

const emit = defineEmits<{
  (e: 'saved'): void
  (e: 'cancel'): void
}>()

const { t } = useI18n()
const { saveChannel, revealToken, probing, probeChannel } = useChannels()

const isNew = computed(() => props.channel === null)

const id = ref(props.channel?.id ?? '')
const name = ref(props.channel?.name ?? '')
const baseUrl = ref(props.channel?.baseUrl ?? '')
const authMode = ref<'bearer' | 'none'>(props.channel?.authMode ?? 'bearer')
const authToken = ref('')
const note = ref(props.channel?.note ?? '')
const engineSupport = ref<string[]>(props.channel
  ? props.channel.scope === 'agent-only' ? [] : [...props.channel.engineSupport]
  : ['claude-code'])
const protocol = ref(props.channel?.protocol ?? 'anthropic')
const tokenVisible = ref(false)
const modelsText = ref((props.channel?.availableModels ?? []).join('\n'))
const codexProviderId = ref(props.channel?.codex?.providerId ?? `monet-${id.value || 'proxy'}`)
const codexManagedProviderIdAuto = ref(!props.channel?.codex?.providerId)

/** 模型映射 env(编辑回显源 = 渠道当前 modelEnv;子组件变更时更新) */
const sourceModelEnv = computed<Record<string, string>>(() => props.channel?.modelEnv ?? {})
/** 子组件构建出的 env 键值(整命名空间替换语义:保存时随 modelEnv 传出) */
const modelEnv = ref<Record<string, string>>({ ...(props.channel?.modelEnv ?? {}) })
function onModelEnvUpdate(env: Record<string, string>) {
  modelEnv.value = env
}

const isVirtual = computed(() => props.channel?.id === APPLE_FM_CHANNEL_ID)
const supportsClaude = computed(() => engineSupport.value.includes('claude-code'))
const supportsCodex = computed(() => engineSupport.value.includes('codex'))

const managedProviderIdPlaceholder = computed(() => `monet-${id.value.trim() || 'proxy'}`)

function onCodexManagedProviderIdInput() {
  codexManagedProviderIdAuto.value = false
}

function toggleEngine(engineId: string) {
  if (engineSupport.value.includes(engineId)) {
    engineSupport.value = engineSupport.value.filter(engine => engine !== engineId)
    return
  }
  engineSupport.value = [...engineSupport.value, engineId]
}

/** 「获取模型列表」:一律用表单当前值直探(新建与编辑同款)。
 *  编辑态若按 id 走渠道文件,用户改了 baseUrl/token/协议还没保存时,
 *  探的仍是磁盘旧值——表现为「切换了配置,检测的还是原渠道」 */
const probeTargetId = computed(() => props.channel?.id ?? id.value)
const modelMapProbing = computed(() => !!probing.value[probeTargetId.value])
async function onProbe() {
  const target = probeTargetId.value.trim()
  if (!target) return
  if (isVirtual.value) {
    await probeChannel(target)
    return
  }
  const url = baseUrl.value.trim().replace(/\/+$/, '')
  if (!url) {
    formError.value = t('settings.channelForm.baseUrlError')
    return
  }
  formError.value = null
  const result = await probeChannel(target, { baseUrl: url, token: authToken.value.trim(), protocol: protocol.value })
  if (result?.models.length) {
    const merged = [...new Set([...parsedModels.value, ...result.models])].sort()
    modelsText.value = merged.join('\n')
  }
}

const parsedModels = computed(() => [...new Set(modelsText.value
  .split(/[,，\n]/)
  .map(model => model.trim())
  .filter(Boolean))].sort())
const modelOptions = computed(() => parsedModels.value)

const saving = ref(false)
const formError = ref<string | null>(null)

const ID_PATTERN = /^[a-zA-Z0-9_-]{1,64}$/

watch(id, () => {
  if (codexManagedProviderIdAuto.value) {
    codexProviderId.value = managedProviderIdPlaceholder.value
  }
})

onMounted(async () => {
  if (!isNew.value && props.channel) {
    const token = await revealToken(props.channel.id)
    if (token) authToken.value = token
  }
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
  if (!isVirtual.value && !baseUrl.value.trim()) {
    formError.value = t('settings.channelForm.baseUrlError')
    return
  }
  if (!isVirtual.value && authMode.value === 'bearer' && !authToken.value.trim()) {
    formError.value = t('settings.channelForm.tokenError')
    return
  }
  if (supportsCodex.value && !codexProviderId.value.trim()) {
    formError.value = t('settings.channelForm.codexProviderError')
    return
  }
  saving.value = true
  try {
    await saveChannel({
      id: trimmedId,
      name: name.value.trim(),
      baseUrl: isVirtual.value ? '' : baseUrl.value.trim().replace(/\/+$/, ''),
      authMode: authMode.value,
      authToken: authToken.value.trim() || undefined,
      note: note.value.trim() || undefined,
      protocol: protocol.value,
      availableModels: parsedModels.value,
      // 整命名空间替换语义:虚拟渠道不传(保持 undefined→null 不动 env);其余传构建后的 env(空对象=清除映射)
      modelEnv: isVirtual.value ? undefined : modelEnv.value,
      engineSupport: [...engineSupport.value],
      codex: supportsCodex.value ? {
        mode: 'managed',
        providerId: codexProviderId.value.trim(),
      } : undefined,
    })
    emit('saved')
  } catch (e) {
    formError.value = String(e)
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
      <input
        v-model="id"
        :disabled="!isNew"
        type="text"
        :placeholder="$t('settings.channelForm.idPlaceholder')"
        class="form-input disabled:opacity-50"
        spellcheck="false"
      />
    </label>

    <label class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.nameLabel') }}</span>
      <input v-model="name" type="text" :placeholder="$t('settings.channelForm.namePlaceholder')" class="form-input" />
    </label>

    <section v-if="!isVirtual" class="engine-binding connection-binding">
      <div class="engine-binding-head">
        <span class="i-carbon-link h-3.5 w-3.5 text-primary" />
        <span>{{ $t('settings.channelForm.connectionTitle') }}</span>
        <span class="ml-auto text-[9px] font-normal text-muted-foreground">{{ $t('settings.channelForm.connectionShared') }}</span>
      </div>

      <label class="form-field">
        <span class="form-label">Base URL <span class="text-muted-foreground font-normal">{{ $t('settings.channelForm.baseUrlHint') }}</span></span>
        <input v-model="baseUrl" type="text" placeholder="https://api.example.com" class="form-input font-mono" spellcheck="false" />
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
          <button type="button" class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors" @click="tokenVisible = !tokenVisible">
            <span :class="tokenVisible ? 'i-carbon-view-off' : 'i-carbon-view'" class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      <label class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.protocolLabel') }}</span>
        <select v-model="protocol" class="form-input">
          <option value="anthropic">Anthropic-compatible</option>
          <option value="openai">OpenAI-compatible</option>
        </select>
        <span class="text-[10px] leading-snug text-muted-foreground/70">{{ $t('settings.channelForm.protocolHint') }}</span>
      </label>

      <label class="form-field">
        <span class="flex items-center justify-between gap-2">
          <span class="form-label">{{ $t('settings.channelForm.modelsLabel') }}</span>
          <button type="button" class="text-[10px] text-primary hover:underline disabled:opacity-50" :disabled="modelMapProbing" @click="onProbe">
            {{ modelMapProbing ? $t('settings.channelForm.modelMap.fetching') : $t('settings.channelForm.modelMap.fetchModels') }}
          </button>
        </span>
        <textarea v-model="modelsText" rows="3" class="form-input resize-y font-mono" placeholder="claude-sonnet-4-5&#10;gpt-5.2" spellcheck="false" />
        <span class="text-[10px] leading-snug text-muted-foreground/70">{{ $t('settings.channelForm.modelsHint') }}</span>
      </label>
    </section>

    <div class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.engineSupportLabel') }}</span>
      <div class="flex items-center gap-1.5">
        <button v-for="engine in [{ id: 'claude-code', label: 'Claude Code' }, { id: 'codex', label: 'Codex' }]" :key="engine.id" type="button" class="engine-chip" :class="{ active: engineSupport.includes(engine.id) }" :aria-pressed="engineSupport.includes(engine.id)" @click="toggleEngine(engine.id)">
          <span class="h-1.5 w-1.5 rounded-full" :class="engine.id === 'codex' ? 'bg-codex' : 'bg-claude'" />
          {{ engine.label }}
        </button>
      </div>
      <span class="text-[10px] leading-snug text-muted-foreground/70">{{ $t('settings.channelForm.engineSupportHint') }}</span>
    </div>

    <section v-if="supportsClaude" class="engine-binding">
      <div class="engine-binding-head">
        <span class="h-1.5 w-1.5 rounded-full bg-claude" />
        <span>Claude Code</span>
        <span class="ml-auto font-mono text-[9px] font-normal text-muted-foreground">Messages</span>
      </div>
      <p class="text-[10px] leading-snug text-muted-foreground/70">{{ $t('settings.channelForm.claudeAdapterHint') }}</p>
      <ChannelModelMap
        :model-env="sourceModelEnv"
        :model-options="modelOptions"
        :probing="modelMapProbing"
        :dom-key="id || 'new'"
        @update:env="onModelEnvUpdate"
        @probe="onProbe"
      />
    </section>

    <section v-if="supportsCodex" class="engine-binding">
      <div class="engine-binding-head">
        <span class="h-1.5 w-1.5 rounded-full bg-codex" />
        <span>Codex</span>
        <span class="ml-auto font-mono text-[9px] font-normal text-muted-foreground">Responses</span>
      </div>
      <label class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.codexProviderLabel') }}</span>
        <input v-model="codexProviderId" type="text" :placeholder="managedProviderIdPlaceholder" class="form-input font-mono" spellcheck="false" @input="onCodexManagedProviderIdInput" />
        <span class="text-[10px] leading-snug text-muted-foreground/70">{{ $t('settings.channelForm.codexAdapterHint') }}</span>
      </label>
    </section>

    <label class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.noteLabel') }}</span>
      <input v-model="note" type="text" :placeholder="$t('common.optional')" class="form-input" />
    </label>

    <p v-if="formError" class="text-xs text-destructive">{{ formError }}</p>

    <div class="flex items-center gap-2 justify-end">
      <button
        class="px-2.5 py-1 text-xs rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
        @click="emit('cancel')"
      >
        {{ $t('common.cancel') }}
      </button>
      <button
        :disabled="saving"
        class="px-2.5 py-1 text-xs rounded-md bg-primary text-primary-foreground hover:shadow-paper transition-shadow disabled:opacity-50"
        @click="onSave"
      >
        {{ saving ? $t('common.saving') : $t('common.save') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.form-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.form-label {
  font-size: 11px;
  color: var(--muted-foreground);
}
.engine-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-height: 24px;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  color: var(--muted-foreground);
  font-size: 11px;
  transition: color 0.15s, border-color 0.15s, background 0.15s;
}
.engine-chip:hover {
  color: var(--foreground);
  background: var(--muted);
}
.engine-chip.active {
  color: var(--foreground);
  border-color: color-mix(in srgb, var(--primary) 55%, var(--border));
  background: color-mix(in srgb, var(--primary) 8%, transparent);
}
.engine-binding {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
}
.engine-binding-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding-bottom: 6px;
  border-bottom: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
  font-size: 11px;
  font-weight: 600;
}
</style>

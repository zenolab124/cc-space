<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
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
const { saveChannel, revealToken, probeResults, probing, probeChannel } = useChannels()

const isNew = computed(() => props.channel === null)

const id = ref(props.channel?.id ?? '')
const name = ref(props.channel?.name ?? '')
const baseUrl = ref(props.channel?.baseUrl ?? '')
const authToken = ref('')
const note = ref(props.channel?.note ?? '')
const engineSupport = ref<string[]>(props.channel?.engineSupport?.length
  ? [...props.channel.engineSupport]
  : ['claude-code'])
const protocol = ref(props.channel?.protocol ?? 'anthropic')
const scope = ref(props.channel?.scope ?? 'full')
const agentModel = ref(props.channel?.agentModel ?? '')
const tokenVisible = ref(false)
const codexMode = ref<'external' | 'managed'>(props.channel?.codex?.mode ?? 'external')
const codexProviderId = ref(props.channel?.codex?.providerId ?? '')
const codexBaseUrl = ref(props.channel?.codex?.baseUrl ?? '')
const codexAuthMode = ref<'bearer' | 'openai' | 'none'>(props.channel?.codex?.authMode ?? 'bearer')
const codexAuthToken = ref('')
const codexTokenVisible = ref(false)
const codexDefaultModel = ref(props.channel?.codex?.defaultModel ?? '')
const codexDefaultEffort = ref(props.channel?.codex?.defaultEffort ?? '')
const codexModelsText = ref((props.channel?.codex?.availableModels ?? []).join(', '))
/** 渠道默认模型(env.ANTHROPIC_MODEL,保存时合并进 modelEnv)与默认思考强度(顶层 effortLevel/ultracode) */
const defaultModel = ref(props.channel?.defaultModel ?? '')
const defaultEffort = ref(props.channel?.defaultEffort ?? '')

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

function toggleEngine(engineId: string) {
  if (engineSupport.value.includes(engineId)) {
    if (engineSupport.value.length > 1) {
      engineSupport.value = engineSupport.value.filter(engine => engine !== engineId)
    }
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
  await probeChannel(target, { baseUrl: url, token: authToken.value.trim(), protocol: protocol.value })
}

const modelOptions = computed(() => {
  const channelId = props.channel?.id ?? id.value
  const probeModels = probeResults.value[channelId]?.models ?? []
  const savedModels = props.channel?.availableModels ?? []
  return [...new Set([...probeModels, ...savedModels])].sort()
})

const saving = ref(false)
const formError = ref<string | null>(null)

const ID_PATTERN = /^[a-zA-Z0-9_-]{1,64}$/

onMounted(async () => {
  if (!isNew.value && props.channel) {
    const [claudeToken, codexToken] = await Promise.all([
      revealToken(props.channel.id, 'claude-code'),
      props.channel.codex ? revealToken(props.channel.id, 'codex') : Promise.resolve(null),
    ])
    if (claudeToken) authToken.value = claudeToken
    if (codexToken) codexAuthToken.value = codexToken
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
  if (!engineSupport.value.length) {
    formError.value = t('settings.channelForm.engineRequired')
    return
  }
  if (supportsClaude.value && !isVirtual.value && !baseUrl.value.trim()) {
    formError.value = t('settings.channelForm.baseUrlError')
    return
  }
  if (supportsClaude.value && !isVirtual.value && protocol.value !== 'openai' && !authToken.value.trim()) {
    formError.value = t('settings.channelForm.tokenError')
    return
  }
  if (supportsCodex.value && !codexProviderId.value.trim()) {
    formError.value = t('settings.channelForm.codexProviderError')
    return
  }
  if (supportsCodex.value && codexMode.value === 'managed' && !codexBaseUrl.value.trim()) {
    formError.value = t('settings.channelForm.codexBaseUrlError')
    return
  }
  if (supportsCodex.value && codexMode.value === 'managed' && codexAuthMode.value === 'bearer' && !codexAuthToken.value.trim()) {
    formError.value = t('settings.channelForm.codexTokenError')
    return
  }
  saving.value = true
  try {
    // 默认模型归主表单持有,保存时合并进映射 env(子组件产出恒不含 ANTHROPIC_MODEL)
    const mergedEnv: Record<string, string> = { ...modelEnv.value }
    delete mergedEnv['ANTHROPIC_MODEL']
    const dm = defaultModel.value.trim()
    if (dm) mergedEnv['ANTHROPIC_MODEL'] = dm
    await saveChannel({
      id: trimmedId,
      name: name.value.trim(),
      baseUrl: !supportsClaude.value || isVirtual.value ? '' : baseUrl.value.trim().replace(/\/+$/, ''),
      authToken: authToken.value.trim() || undefined,
      note: note.value.trim() || undefined,
      protocol: protocol.value,
      scope: scope.value,
      agentModel: agentModel.value.trim() || undefined,
      availableModels: modelOptions.value.length > 0 ? modelOptions.value : undefined,
      // 整命名空间替换语义:虚拟渠道不传(保持 undefined→null 不动 env);其余传构建后的 env(空对象=清除映射)
      modelEnv: isVirtual.value ? undefined : mergedEnv,
      // 替换语义:空串=清除默认(移除顶层 effortLevel/ultracode);虚拟渠道不传
      defaultEffort: isVirtual.value ? undefined : defaultEffort.value,
      engineSupport: [...engineSupport.value],
      codex: supportsCodex.value ? {
        mode: codexMode.value,
        providerId: codexProviderId.value.trim(),
        baseUrl: codexBaseUrl.value.trim().replace(/\/+$/, '') || undefined,
        authMode: codexAuthMode.value,
        authToken: codexAuthToken.value.trim() || undefined,
        defaultModel: codexDefaultModel.value.trim() || undefined,
        defaultEffort: codexDefaultEffort.value || undefined,
        availableModels: codexModelsText.value
          .split(/[,，\n]/)
          .map(model => model.trim())
          .filter(Boolean),
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

    <div class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.engineSupportLabel') }}</span>
      <div class="flex items-center gap-1.5">
        <button
          v-for="engine in [{ id: 'claude-code', label: 'Claude Code' }, { id: 'codex', label: 'Codex' }]"
          :key="engine.id"
          type="button"
          class="engine-chip"
          :class="{ active: engineSupport.includes(engine.id) }"
          :aria-pressed="engineSupport.includes(engine.id)"
          @click="toggleEngine(engine.id)"
        >
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
        <span class="ml-auto font-mono text-[9px] font-normal text-muted-foreground">settings / env</span>
      </div>
    <label class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.protocolLabel') }}</span>
      <select v-model="protocol" class="form-input">
        <option value="anthropic">Anthropic Messages API</option>
        <option value="openai">OpenAI Chat Completions</option>
      </select>
    </label>

    <label class="form-field">
      <span class="form-label">Base URL <span class="text-muted-foreground font-normal">{{ $t('settings.channelForm.baseUrlHint') }}</span></span>
      <input
        v-model="baseUrl"
        type="text"
        placeholder="https://api.example.com/anthropic"
        class="form-input font-mono"
        spellcheck="false"
      />
    </label>

    <div class="form-field">
      <span class="form-label">Auth Token <span v-if="protocol === 'openai'" class="text-muted-foreground/60 font-normal italic">{{ $t('settings.channelForm.tokenOptional') }}</span></span>
      <div class="relative">
        <input
          v-model="authToken"
          :type="tokenVisible ? 'text' : 'password'"
          placeholder="sk-…"
          class="form-input font-mono w-full pr-8"
          autocomplete="off"
        />
        <button
          type="button"
          class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
          @click="tokenVisible = !tokenVisible"
        >
          <span :class="tokenVisible ? 'i-carbon-view-off' : 'i-carbon-view'" class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>

    <label class="form-field">
      <span class="form-label">{{ $t('settings.channelForm.scopeLabel') }}</span>
      <select v-model="scope" class="form-input">
        <option value="full">{{ $t('settings.channelForm.scopeFullHint') }}</option>
        <option value="agent-only">{{ $t('settings.channelForm.scopeAgentOnlyHint') }}</option>
      </select>
    </label>

    <!-- 渠道运行预设:默认模型 + 默认思考强度(会话解析链的渠道层;agent-only 渠道不进会话链,隐藏) -->
    <div v-if="scope === 'full' && !isVirtual" class="grid grid-cols-2 gap-2">
      <div class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.defaultModelLabel') }} <span class="text-muted-foreground/60 font-normal italic">{{ $t('common.optional') }}</span></span>
        <input
          v-model="defaultModel"
          type="text"
          :list="`default-model-opts-${id}`"
          :placeholder="$t('settings.channelForm.defaultModelPlaceholder')"
          class="form-input font-mono"
          spellcheck="false"
        />
        <datalist :id="`default-model-opts-${id}`">
          <option v-for="m in modelOptions" :key="m" :value="m" />
        </datalist>
      </div>
      <div class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.defaultEffortLabel') }} <span class="text-muted-foreground/60 font-normal italic">{{ $t('common.optional') }}</span></span>
        <select v-model="defaultEffort" class="form-input">
          <option value="">{{ $t('settings.channelForm.defaultEffortNone') }}</option>
          <option value="low">Low</option>
          <option value="medium">Medium</option>
          <option value="high">High</option>
          <option value="xhigh">xHigh</option>
          <option value="max">Max</option>
          <option value="ultracode">Ultracode</option>
        </select>
      </div>
      <p class="text-[10px] text-muted-foreground/60 leading-snug col-span-2 -mt-1">{{ $t('settings.channelForm.defaultsHint') }}</p>
    </div>

    <div class="form-field">
      <span class="form-label">Agent Model <span class="text-muted-foreground/60 font-normal italic">{{ $t('common.optional') }}</span></span>
      <input
        v-model="agentModel"
        type="text"
        :list="`agent-model-opts-${id}`"
        placeholder="e.g. gpt-4o-mini"
        class="form-input font-mono"
        spellcheck="false"
      />
      <datalist :id="`agent-model-opts-${id}`">
        <option v-for="m in modelOptions" :key="m" :value="m" />
      </datalist>
      <span class="text-[10px] text-muted-foreground/70">{{ $t('settings.channelForm.agentModelHint') }}</span>
    </div>

    <!-- 高级选项 · 模型映射(虚拟渠道隐藏) -->
    <ChannelModelMap
      v-if="!isVirtual"
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
        <span class="ml-auto font-mono text-[9px] font-normal text-muted-foreground">model_provider / Responses</span>
      </div>

      <label class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.codexModeLabel') }}</span>
        <select v-model="codexMode" class="form-input">
          <option value="external">{{ $t('settings.channelForm.codexModeExternal') }}</option>
          <option value="managed">{{ $t('settings.channelForm.codexModeManaged') }}</option>
        </select>
        <span class="text-[10px] leading-snug text-muted-foreground/70">
          {{ $t(codexMode === 'external' ? 'settings.channelForm.codexModeExternalHint' : 'settings.channelForm.codexModeManagedHint') }}
        </span>
      </label>

      <label class="form-field">
        <span class="form-label">Provider ID</span>
        <input v-model="codexProviderId" type="text" :placeholder="codexMode === 'managed' ? `monet-${id || 'proxy'}` : 'openai / proxy / ollama'" class="form-input font-mono" spellcheck="false" />
      </label>

      <template v-if="codexMode === 'managed'">
        <label class="form-field">
          <span class="form-label">Base URL <span class="text-muted-foreground font-normal">{{ $t('settings.channelForm.baseUrlHint') }}</span></span>
          <input v-model="codexBaseUrl" type="text" placeholder="https://api.example.com/v1" class="form-input font-mono" spellcheck="false" />
        </label>

        <label class="form-field">
          <span class="form-label">{{ $t('settings.channelForm.codexAuthLabel') }}</span>
          <select v-model="codexAuthMode" class="form-input">
            <option value="bearer">Bearer Token</option>
            <option value="openai">{{ $t('settings.channelForm.codexAuthOpenai') }}</option>
            <option value="none">{{ $t('settings.channelForm.codexAuthNone') }}</option>
          </select>
        </label>

        <div v-if="codexAuthMode === 'bearer'" class="form-field">
          <span class="form-label">Bearer Token</span>
          <div class="relative">
            <input v-model="codexAuthToken" :type="codexTokenVisible ? 'text' : 'password'" placeholder="sk-…" class="form-input font-mono w-full pr-8" autocomplete="off" />
            <button type="button" class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground transition-colors hover:text-foreground" @click="codexTokenVisible = !codexTokenVisible">
              <span :class="codexTokenVisible ? 'i-carbon-view-off' : 'i-carbon-view'" class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </template>

      <div class="grid grid-cols-2 gap-2">
        <label class="form-field">
          <span class="form-label">{{ $t('settings.channelForm.defaultModelLabel') }} <span class="text-muted-foreground/60 font-normal italic">{{ $t('common.optional') }}</span></span>
          <input v-model="codexDefaultModel" type="text" placeholder="gpt-5.6-sol" class="form-input font-mono" spellcheck="false" />
        </label>
        <label class="form-field">
          <span class="form-label">{{ $t('settings.channelForm.defaultEffortLabel') }} <span class="text-muted-foreground/60 font-normal italic">{{ $t('common.optional') }}</span></span>
          <select v-model="codexDefaultEffort" class="form-input">
            <option value="">{{ $t('settings.channelForm.defaultEffortNone') }}</option>
            <option value="low">Low</option>
            <option value="medium">Medium</option>
            <option value="high">High</option>
            <option value="xhigh">xHigh</option>
          </select>
        </label>
      </div>

      <label class="form-field">
        <span class="form-label">{{ $t('settings.channelForm.codexModelsLabel') }} <span class="text-muted-foreground/60 font-normal italic">{{ $t('common.optional') }}</span></span>
        <textarea v-model="codexModelsText" rows="2" class="form-input resize-none font-mono" placeholder="gpt-5.6-sol, gpt-5.6-terra" spellcheck="false" />
        <span class="text-[10px] leading-snug text-muted-foreground/70">{{ $t('settings.channelForm.codexModelsHint') }}</span>
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

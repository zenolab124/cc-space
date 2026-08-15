import { computed, type ComputedRef, type Ref } from 'vue'
import {
  MODELS,
  officialRoleItems,
  DEFAULT_CONTEXT,
  getContextWindow,
  type ModelInfo,
} from '@/utils/modelContext'
import {
  MODEL_ROLES,
  ROLE_ALIAS,
  CUSTOM_MODEL_OPTION,
  CUSTOM_MODEL_NAME,
  roleModelKey,
  roleNameKey,
  valueHasOneM,
  hasAnyMapping,
  stripOneM,
} from '@/utils/modelEnv'
import { useChannels, OFFICIAL_CHANNEL_ID, OFFICIAL_DIRECT_CHANNEL_ID } from '@/composables/useChannels'

const EXTENDED_CONTEXT = 1_000_000

/**
 * 按渠道产出模型下拉候选项。
 *
 * 输入:渠道 id 的 Ref。null / 'official' 视为官方渠道。
 * 输出:items —— 供运行配置胶囊(RunConfigCapsule)模型列消费的 ModelInfo[]。
 *
 * 三分支:
 *   1. 官方渠道:OFFICIAL_ROLE_ITEMS(四角色主区) + MODELS 全量标 legacy 沉底
 *      (原本 legacy 与非 legacy 项在官方角色语境下统一视为「钉版本」沉底区,
 *       复用模型列现有 legacy 分割线渲染)。
 *   2. 第三方:角色映射作为快捷入口排在前面；渠道模型目录中未被角色占用的模型
 *      全部以真实 ID 直传。映射只增加语义，不再充当模型过滤器。
 *   3. 旧渠道既无目录也无映射时，回退 MODELS，保持存量行为。
 */
export function useModelOptions(channelId: Ref<string | null>): {
  items: ComputedRef<ModelInfo[]>
} {
  const { channels } = useChannels()

  const isOfficial = computed(
    () => !channelId.value || channelId.value === OFFICIAL_CHANNEL_ID || channelId.value === OFFICIAL_DIRECT_CHANNEL_ID,
  )

  /** 当前渠道对象(第三方) */
  const channel = computed(() =>
    isOfficial.value ? null : channels.value.find(c => c.id === channelId.value) ?? null,
  )

  const items = computed<ModelInfo[]>(() => {
    // 分支 1:官方渠道 —— 角色主区 + 钉版本沉底
    if (isOfficial.value) {
      const roles = officialRoleItems()
      const pinned = MODELS.map<ModelInfo>(m => ({ ...m, legacy: true }))
      return [...roles, ...pinned]
    }

    const currentChannel = channel.value
    const modelEnv = currentChannel?.modelEnv ?? {}
    const catalog = currentChannel?.availableModels ?? []

    // 分支 3:旧渠道无任何模型信息 —— 回退内置清单。
    if (!hasAnyMapping(modelEnv) && catalog.length === 0) {
      return MODELS
    }

    // 分支 2:角色快捷入口 + 未映射模型真实 ID。
    const result: ModelInfo[] = []
    const mappedTargets = new Set<string>()
    const emittedIds = new Set<string>()
    for (const role of MODEL_ROLES) {
      const modelVal = modelEnv[roleModelKey(role)]?.trim()
      if (!modelVal) continue
      mappedTargets.add(stripOneM(modelVal).toLowerCase())
      const nameVal = modelEnv[roleNameKey(role)]?.trim()
      const alias = ROLE_ALIAS[role]
      result.push({
        // 裸 alias:CLI 会经渠道 env 把 alias 重定向到映射模型,1M 由映射值自带
        id: alias,
        label: nameVal || modelVal,
        contextWindow: valueHasOneM(modelVal) ? EXTENDED_CONTEXT : DEFAULT_CONTEXT,
        // 来源角色槽:UI 标注该映射模型伪装的等级(自定义槽不伪装,不标)
        mappedRole: role,
      })
      emittedIds.add(alias.toLowerCase())
    }

    // 自定义槽殿后:值直接进 CLI /model 菜单并通过校验
    const customVal = modelEnv[CUSTOM_MODEL_OPTION]?.trim()
    if (customVal) {
      const customName = modelEnv[CUSTOM_MODEL_NAME]?.trim()
      result.push({
        id: customVal,
        label: customName || customVal,
        contextWindow: valueHasOneM(customVal) ? EXTENDED_CONTEXT : DEFAULT_CONTEXT,
      })
      emittedIds.add(customVal.toLowerCase())
    }

    for (const modelId of catalog) {
      const trimmed = modelId.trim()
      if (!trimmed) continue
      const normalizedId = trimmed.toLowerCase()
      if (emittedIds.has(normalizedId) || mappedTargets.has(stripOneM(trimmed).toLowerCase())) continue
      result.push({
        id: trimmed,
        label: trimmed,
        contextWindow: getContextWindow(trimmed),
      })
      emittedIds.add(normalizedId)
    }

    return result
  })

  return { items }
}

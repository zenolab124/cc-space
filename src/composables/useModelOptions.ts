import { computed, type ComputedRef, type Ref } from 'vue'
import {
  MODELS,
  officialRoleItems,
  DEFAULT_CONTEXT,
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
 *   2. 第三方且有映射(modelEnv 存在任一 ANTHROPIC_DEFAULT_*_MODEL 或 CUSTOM_MODEL_OPTION):
 *      每个已映射角色 → 裸 alias id(CLI 经渠道 env 重定向,无需 [1m]),
 *      label 取 _NAME 值 ?? 映射模型值,容量按映射值含 [1m] 与否;自定义槽殿后。
 *   3. 第三方无映射:回退 MODELS 全量(现状行为,渐进增强不破坏存量渠道)。
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

    const modelEnv = channel.value?.modelEnv ?? {}

    // 分支 3:第三方无映射 —— 回退 MODELS 全量(不破坏存量渠道)
    if (!hasAnyMapping(modelEnv)) {
      return MODELS
    }

    // 分支 2:第三方有映射 —— 逐角色 + 自定义槽
    const result: ModelInfo[] = []
    for (const role of MODEL_ROLES) {
      const modelVal = modelEnv[roleModelKey(role)]?.trim()
      if (!modelVal) continue
      const nameVal = modelEnv[roleNameKey(role)]?.trim()
      result.push({
        // 裸 alias:CLI 会经渠道 env 把 alias 重定向到映射模型,1M 由映射值自带
        id: ROLE_ALIAS[role],
        label: nameVal || modelVal,
        contextWindow: valueHasOneM(modelVal) ? EXTENDED_CONTEXT : DEFAULT_CONTEXT,
        // 来源角色槽:UI 标注该映射模型伪装的等级(自定义槽不伪装,不标)
        mappedRole: role,
      })
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
    }

    return result
  })

  return { items }
}

import type { AssistantResponseMeta } from '@/utils/assistantResponse'
import type { ConversationRecord } from './types'

export type EngineAccent = 'claude' | 'codex' | 'primary'

/** 两种源数据进入共享轮次渲染器前的稳定视图契约。 */
export interface ConversationTurnView {
  dayLabel: string | null
  timeLabel: string | null
  user: {
    visible: boolean
    sticky: boolean
    hidden: boolean
  }
  response: {
    visible: boolean
    meta: AssistantResponseMeta | null
    showFooter: boolean
    speaker?: string
    accent: EngineAccent
  }
  lazy: boolean
}

export interface EnginePresentation {
  accent: EngineAccent
  displayName: string
  /** 是否允许过程文本与推理内容进入会话视图。 */
  showThoughtProcess: boolean
}

/** 将标准引擎的轮次记录收束为共享回复框消费的元信息。 */
export function engineResponseMeta(
  records: ConversationRecord[],
  fallbackModel: string | null,
): Pick<AssistantResponseMeta, 'model' | 'usage' | 'calls'> {
  const responseRecords = records.filter(record => record.role !== 'user')
  const modelValue = [...responseRecords]
    .reverse()
    .map(record => record.sourceMeta.model)
    .find(model => typeof model === 'string' && !!model.trim())
  const usage = [...responseRecords].reverse().find(record => record.usage)?.usage
  return {
    model: typeof modelValue === 'string' ? modelValue : fallbackModel,
    usage: usage ? {
      input_tokens: usage.inputTokens,
      output_tokens: usage.outputTokens,
      cache_creation_input_tokens: usage.cacheCreationInputTokens ?? 0,
      cache_read_input_tokens: usage.cachedInputTokens ?? 0,
    } : null,
    calls: usage ? 1 : 0,
  }
}

const PRESENTATIONS: Record<string, Omit<EnginePresentation, 'displayName'>> = {
  claude: {
    accent: 'claude',
    showThoughtProcess: true,
  },
  'claude-code': {
    accent: 'claude',
    showThoughtProcess: true,
  },
  codex: {
    accent: 'codex',
    showThoughtProcess: false,
  },
}

/**
 * 会话展示差异的单一入口。新增引擎默认使用产品主色；只有需要独立身份色或
 * 特殊展示策略时才追加一项，避免在组件树里散落 engineId 判断。
 */
export function resolveEnginePresentation(
  engineId: string | null | undefined,
  displayName: string | null | undefined,
): EnginePresentation {
  const id = engineId?.toLowerCase() ?? ''
  const configured = PRESENTATIONS[id]
  return {
    accent: configured?.accent ?? 'primary',
    displayName: displayName?.trim() || engineId?.trim() || 'Agent',
    showThoughtProcess: configured?.showThoughtProcess ?? true,
  }
}

import { hasReportedUsage, type TokenUsage } from '@/types'

export interface AssistantResponseSample {
  model?: string | null
  usage?: Partial<TokenUsage> | null
}

export interface AssistantResponseSummary {
  model: string | null
  usage: TokenUsage | null
  calls: number
}

export interface AssistantResponseMeta extends AssistantResponseSummary {
  completedText?: string | null
  completedFull?: string | null
  tier?: string | null
}

/** 将一次用户轮次中的多次 assistant API 调用收束为一份展示摘要。 */
export function summarizeAssistantResponse(
  samples: readonly AssistantResponseSample[],
): AssistantResponseSummary {
  let model: string | null = null
  let calls = 0
  const usage: TokenUsage = {
    input_tokens: 0,
    output_tokens: 0,
    cache_creation_input_tokens: 0,
    cache_read_input_tokens: 0,
  }

  for (const sample of samples) {
    if (sample.model && sample.model !== '<synthetic>') model = sample.model
    if (!hasReportedUsage(sample.usage)) continue
    calls += 1
    usage.input_tokens += sample.usage?.input_tokens ?? 0
    usage.output_tokens += sample.usage?.output_tokens ?? 0
    usage.cache_creation_input_tokens += sample.usage?.cache_creation_input_tokens ?? 0
    usage.cache_read_input_tokens += sample.usage?.cache_read_input_tokens ?? 0
  }

  return {
    model,
    usage: calls > 0 ? usage : null,
    calls,
  }
}

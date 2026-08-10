import type { ContentBlock } from '@/types'
import type { ToolResultAttachment } from '@/engines/types'

export const INLINE_RESULT_TOOLS = new Set(['Bash', 'WebSearch'])

export function usesInlineToolResult(name: string): boolean {
  return INLINE_RESULT_TOOLS.has(name) || name.startsWith('mcp__')
}

export interface ToolResultData {
  content: string | ContentBlock[]
  is_error: boolean
  attachments?: ToolResultAttachment[]
  /** tool_result 所在 record 的 uuid;嵌套图片(如 MCP 截图)拼 ccimg 协议 URL 用。流式内存路径无落盘 uuid 故可空 */
  recordUuid?: string | null
}

export function flattenResultText(content: string | ContentBlock[]): string {
  if (typeof content === 'string') return content
  return content
    .filter((b): b is Extract<ContentBlock, { type: 'text' }> => b.type === 'text')
    .map(b => b.text)
    .join('\n')
}

export function filterConsumedResults(blocks: ContentBlock[]): ContentBlock[] {
  const inlineIds = new Set<string>()
  for (const b of blocks) {
    if (b.type === 'tool_use' && usesInlineToolResult((b as { name: string }).name)) {
      inlineIds.add((b as { id: string }).id)
    }
  }
  if (inlineIds.size === 0) return blocks
  return blocks.filter(b => {
    if (b.type === 'tool_result') {
      return !inlineIds.has((b as { tool_use_id: string }).tool_use_id)
    }
    return true
  })
}

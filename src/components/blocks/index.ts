import type { Component } from 'vue'
import type { ContentBlock } from '@/types'
import BlockText from './BlockText.vue'
import BlockThinking from './BlockThinking.vue'
import BlockRedactedThinking from './BlockRedactedThinking.vue'
import BlockToolUse from './BlockToolUse.vue'
import BlockToolResult from './BlockToolResult.vue'
import BlockImage from './BlockImage.vue'
import BlockDocument from './BlockDocument.vue'
import BlockSkillPrompt from './BlockSkillPrompt.vue'
import BlockIdeOpenedFile from './BlockIdeOpenedFile.vue'
import BlockIdeSelection from './BlockIdeSelection.vue'
import BlockSystemReminder from './BlockSystemReminder.vue'
import BlockTaskNotification from './BlockTaskNotification.vue'
import BlockPersistedOutput from './BlockPersistedOutput.vue'
import BlockToolUseError from './BlockToolUseError.vue'
import BlockCommandName from './BlockCommandName.vue'
import BlockSystemEvent from './BlockSystemEvent.vue'
import BlockImageMeta from './BlockImageMeta.vue'
import BlockUnknown from './BlockUnknown.vue'

export {
  BlockText,
  BlockThinking,
  BlockRedactedThinking,
  BlockToolUse,
  BlockToolResult,
  BlockImage,
  BlockDocument,
  BlockSkillPrompt,
  BlockIdeOpenedFile,
  BlockIdeSelection,
  BlockSystemReminder,
  BlockTaskNotification,
  BlockPersistedOutput,
  BlockToolUseError,
  BlockCommandName,
  BlockSystemEvent,
  BlockImageMeta,
  BlockUnknown,
}

/** ContentBlock.type → 渲染组件的映射表 */
const BLOCK_MAP: Record<string, Component> = {
  text: BlockText,
  thinking: BlockThinking,
  redacted_thinking: BlockRedactedThinking,
  tool_use: BlockToolUse,
  tool_result: BlockToolResult,
  image: BlockImage,
  document: BlockDocument,
  skill_prompt: BlockSkillPrompt,
  ide_opened_file: BlockIdeOpenedFile,
  ide_selection: BlockIdeSelection,
  'system-reminder': BlockSystemReminder,
  'task-notification': BlockTaskNotification,
  'persisted-output': BlockPersistedOutput,
  'user-prompt-submit-hook': BlockPersistedOutput,
  'tool_use_error': BlockToolUseError,
  'command-name': BlockCommandName,
  'command-args': BlockCommandName,
  'local-command-stdout': BlockCommandName,
  'system-event': BlockSystemEvent,
  'image-meta': BlockImageMeta,
}

const SYSTEM_BLOCK_TYPES = new Set([
  'system-reminder', 'task-notification', 'persisted-output',
  'user-prompt-submit-hook', 'tool_use_error', 'ide_opened_file',
  'ide_selection', 'command-name', 'command-args', 'local-command-stdout',
  'system-event', 'image-meta',
])

export function isSystemBlock(type: string): boolean {
  return SYSTEM_BLOCK_TYPES.has(type)
}

/** 按 ContentBlock.type 解析到对应 Block 组件,未知类型走 BlockUnknown 兜底 */
export function resolveBlock(block: ContentBlock): Component {
  return BLOCK_MAP[block.type] ?? BlockUnknown
}

<script setup lang="ts">
/**
 * 系统会话查看器：内置 Agent / routine 落盘会话的只读浮层。
 * 会话不在项目列表里（六处扫描面软屏蔽），不走 SessionDetail 的全局选中链，
 * 按 agent 落盘目录 + sessionId 直读 JSONL，渲染层复用 MessageBlock。
 */
import { ref, onMounted, computed, provide } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import MessageBlock from './MessageBlock.vue'
import ContentBlockList from './ContentBlockList.vue'
import { shortId } from '@/types'
import type { ToolResultData } from '@/utils/toolPair'
import { buildAsyncLedger } from '@/composables/useAsyncTasks'
import { TOOL_EXECUTION_CONTEXT } from '@/composables/useToolDisplay'
import type { SessionRecord, ContentBlock } from '@/types'

const props = defineProps<{
  sessionId: string
}>()

const emit = defineEmits<{ close: [] }>()

const { t } = useI18n()

interface AgentSessionDir {
  dirName: string
  path: string
  exists: boolean
}

const records = ref<SessionRecord[]>([])
const loading = ref(true)
const error = ref('')
const dirPath = ref('')

onMounted(async () => {
  try {
    const dir = await invoke<AgentSessionDir>('get_agent_session_dir')
    dirPath.value = dir.path
    records.value = await invoke<SessionRecord[]>('get_session_records', {
      projectId: dir.dirName,
      sessionId: props.sessionId,
    })
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
})

type MessageRecord = Extract<SessionRecord, { type: 'user' | 'assistant' }>

/** content 兼容 string / blocks 双形态，统一为 blocks */
function contentBlocks(r: MessageRecord): ContentBlock[] {
  const content = r.message?.content
  if (!content) return []
  if (typeof content === 'string') return [{ type: 'text', text: content } as ContentBlock]
  return content as ContentBlock[]
}

/** tool_result 配对表：工具卡内联输出用（与 SessionDetail 同一 inject 契约，records-only）*/
const toolResultMap = computed(() => {
  const map = new Map<string, ToolResultData>()
  for (const r of records.value) {
    if (r.type !== 'user' || !r.message) continue
    const content = r.message.content
    if (typeof content === 'string') continue
    for (const b of content) {
      if (b.type === 'tool_result') {
        const tr = b as Extract<ContentBlock, { type: 'tool_result' }>
        map.set(tr.tool_use_id, { content: tr.content, is_error: tr.is_error, recordUuid: r.uuid })
      }
    }
  }
  return map
})
const asyncToolStates = computed(() => {
  const states = new Map<string, ReturnType<typeof buildAsyncLedger>[number]['state']>()
  for (const item of buildAsyncLedger(records.value, [], false)) {
    if (item.toolUseId) states.set(item.toolUseId, item.state)
  }
  return states
})

provide('toolResultMap', toolResultMap)
provide(TOOL_EXECUTION_CONTEXT, {
  results: toolResultMap,
  asyncStates: asyncToolStates,
})

/** 纯 tool_result 的 user 记录不单独渲染——结果已配对进对应工具卡 */
function isToolResultOnly(r: MessageRecord): boolean {
  if (r.type !== 'user') return false
  const blocks = contentBlocks(r)
  return blocks.length > 0 && blocks.every((b) => b.type === 'tool_result')
}

const messages = computed<MessageRecord[]>(() =>
  records.value.filter(
    (r): r is MessageRecord =>
      (r.type === 'user' || r.type === 'assistant') && !isToolResultOnly(r as MessageRecord),
  ),
)

/** user 的 text 内容按纯文本渲染：prompt 里的 <system-reminder> 等标签
 *  走 markdown（html:true）会被当 HTML 吞掉，pre-wrap 保证原文可见 */
function isPlainTextBlock(r: MessageRecord, block: ContentBlock): boolean {
  return r.type === 'user' && block.type === 'text'
}

function openDir() {
  if (dirPath.value) invoke('open_in_finder', { path: dirPath.value })
}
</script>

<template>
  <div
    class="fixed inset-0 z-70 grid place-items-center"
    style="background: rgba(70, 45, 20, 0.18)"
    @mousedown.self="emit('close')"
  >
    <div class="w-[760px] max-w-[92vw] max-h-[82vh] rounded-lg bg-popover border border-border shadow-paper-lifted flex flex-col">
      <div class="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <h3 class="text-sm font-medium flex items-center gap-2">
          {{ $t('common.systemSession') }}
          <span class="font-mono text-[11px] text-muted-foreground">{{ shortId(sessionId) }}</span>
        </h3>
        <div class="flex items-center gap-3">
          <button
            class="text-[11px] text-muted-foreground hover:text-foreground transition-colors"
            @click="openDir"
          >{{ $t('common.revealDir') }}</button>
          <button
            class="i-carbon-close w-4 h-4 text-muted-foreground hover:text-foreground transition-colors"
            @click="emit('close')"
          />
        </div>
      </div>

      <div class="flex-1 overflow-auto px-4 py-3">
        <div v-if="loading" class="text-xs text-muted-foreground py-8 text-center">
          {{ $t('common.loading') }}
        </div>
        <div v-else-if="error" class="text-xs text-destructive py-8 text-center">
          {{ error }}
        </div>
        <div v-else-if="!messages.length" class="text-xs text-muted-foreground py-8 text-center">
          {{ $t('common.systemSessionEmpty') }}
        </div>
        <template v-else>
          <div v-for="(r, i) in messages" :key="r.uuid || i" class="mb-4">
            <div
              class="text-xs font-medium mb-1"
              :class="r.type === 'assistant' ? 'text-claude' : 'text-muted-foreground'"
            >
              {{ r.type === 'assistant' ? $t('session.claude') : $t('common.systemPrompt') }}
            </div>
            <div class="pl-3 border-l-2" :class="r.type === 'assistant' ? 'border-claude/40' : 'border-border'">
              <ContentBlockList
                v-if="r.type === 'assistant'"
                :blocks="contentBlocks(r)"
                :record-uuid="r.uuid"
              />
              <template v-else v-for="(block, bi) in contentBlocks(r)" :key="bi">
                <div
                  v-if="isPlainTextBlock(r, block)"
                  class="text-sm whitespace-pre-wrap break-words"
                >{{ (block as any).text }}</div>
                <MessageBlock
                  v-else
                  :block="block"
                  :record-uuid="r.uuid"
                />
              </template>
            </div>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

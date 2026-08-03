<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionRecord, ContentBlock } from '@/types'
import type { ChannelMark } from '@/composables/useSessionSettings'
import { useToolDisplayMode } from '@/composables/useToolDisplay'
import { joinsToolRun, segmentToolBlocks, type ToolUseBlock } from '@/utils/toolDisplay'
import type { AssistantResponseMeta } from '@/utils/assistantResponse'
import AssistantResponseFrame from './AssistantResponseFrame.vue'
import ContentBlockList from './ContentBlockList.vue'
import ToolProcessGroup from './ToolProcessGroup.vue'
import SystemEventRow from './SystemEventRow.vue'
import MsgClamp from './MsgClamp.vue'
import UserMsgContent from './UserMsgContent.vue'
import DividerMark from './DividerMark.vue'

// 消息组渲染:一个用户消息 + 后续回复(assistant/system)。抽出后被 SessionDetail 三处调用:
// (1) 虚拟化 items 循环 (2) shouldVirtualize=false 全铺 v-for (3) 末组豁免独立铺。
// 逻辑与原 SessionDetail.vue L2224-2332 完全等价,仅把外层 v-for 的 div 交回调用方处理。

type VisibleRecord = Extract<SessionRecord, { type: 'user' | 'assistant' | 'system' }>
interface MsgGroup {
  user: VisibleRecord | null
  responses: VisibleRecord[]
}

const props = defineProps<{
  group: MsgGroup
  gi: number
  dayLabel?: string | null
  timeLabel?: string | null
  /** 自绘吸顶层正在展示本组用户消息:文档流原件隐形但占位,避免临界滚动区双显 */
  hideUser?: boolean
  responseMeta?: AssistantResponseMeta | null
  channelMarksByUuid: Map<string | null, ChannelMark[]>
  modelSwitchName: (record: any) => string | null
  isModelCommandRecord: (record: any) => boolean
  isSystemOnlyUser: (record: any) => boolean
  userHasVisibleContent: (record: any) => boolean
  contentBlocks: (record: any) => ContentBlock[]
  channelMarkLabel: (mark: ChannelMark) => string
  /** 末组保持整体挂载时，对其中已落账响应启用细粒度按需渲染。 */
  granularVisibility?: boolean
}>()

const { t: _t } = useI18n() // 保持导入以便模板 $t 可用
const { toolDisplayMode } = useToolDisplayMode()
const hasAssistantResponses = computed(() => props.group.responses.some(record => record.type === 'assistant'))

interface ResponsePart {
  record: VisibleRecord
  index: number
  blocks: ContentBlock[]
}

interface ResponseEntry {
  key: string
  parts: ResponsePart[]
  blocks: ContentBlock[]
  grouped: boolean
}

function hasChannelMark(record: VisibleRecord): boolean {
  return !!record.uuid && (props.channelMarksByUuid.get(record.uuid)?.length ?? 0) > 0
}

function isPureToolProcess(blocks: ContentBlock[]): boolean {
  return blocks.length > 0 && segmentToolBlocks(blocks).every(segment => segment.kind === 'tools')
}

function toolsOf(blocks: ContentBlock[]): ToolUseBlock[] {
  return segmentToolBlocks(blocks).flatMap(segment => segment.kind === 'tools' ? segment.tools : [])
}

const responseEntries = computed<ResponseEntry[]>(() => {
  const entries: ResponseEntry[] = []
  let pending: ResponseEntry | null = null

  function flush() {
    if (pending) entries.push(pending)
    pending = null
  }

  props.group.responses.forEach((record, index) => {
    const blocks = record.type === 'assistant' ? props.contentBlocks(record) : []
    if (record.type !== 'assistant') {
      flush()
      entries.push({
        key: record.uuid ?? `system:${index}`,
        parts: [{ record, index, blocks }],
        blocks,
        grouped: false,
      })
      return
    }

    const canJoin = toolDisplayMode.value === 'grouped'
      && pending?.parts.every(part => part.record.type === 'assistant')
      && isPureToolProcess(pending.blocks)
      && isPureToolProcess(blocks)
      && !hasChannelMark(pending.parts[pending.parts.length - 1].record)
      && joinsToolRun(pending.blocks, blocks)

    if (canJoin && pending) {
      pending.parts.push({ record, index, blocks })
      pending.blocks.push(...blocks)
      pending.grouped = true
    } else {
      flush()
      pending = {
        key: record.uuid ?? `assistant:${index}`,
        parts: [{ record, index, blocks }],
        blocks: [...blocks],
        grouped: false,
      }
    }
  })
  flush()
  return entries
})
</script>

<template>
  <!-- 跨天分隔:这轮起进入新的一天(首组标会话起始日) -->
  <DividerMark v-if="dayLabel" :label="dayLabel" class="mt-2 mb-0.5" />
  <!-- 用户消息:有 AI 回复时吸顶,无回复的短轮次不启用(减少 sticky 元素数量) -->
  <!-- /model 切换成功(stdout 事实源):渲染成与渠道切换同款的配置分界横线 -->
  <DividerMark
    v-if="group.user && group.user.type === 'user' && modelSwitchName(group.user)"
    icon="i-carbon-model-alt"
    :label="$t('session.modelSwitchMark', { name: modelSwitchName(group.user) })"
  />
  <!-- /model 命令记录本身:静默(事件由上面的 stdout 横线承载;取消选择时无 stdout,不留痕) -->
  <template v-else-if="group.user && group.user.type === 'user' && isModelCommandRecord(group.user)" />
  <!-- 纯系统注入(无真实用户输入):降级为系统注解样式 -->
  <div v-else-if="group.user && group.user.type === 'user' && isSystemOnlyUser(group.user)" class="pl-3">
    <ContentBlockList
      :blocks="contentBlocks(group.user as any)"
      :record-uuid="group.user.uuid"
    />
  </div>
  <!-- 正常用户消息(全空白内容不渲染空卡壳) -->
  <div
    v-else-if="group.user && group.user.type === 'user' && userHasVisibleContent(group.user)"
    :class="[
      group.responses.some(r => r.type === 'assistant') ? 'user-msg-sticky' : '',
      hideUser ? 'user-msg-ghost' : '',
    ]"
  >
    <div class="flex gap-3">
      <div class="w-0.5 shrink-0 rounded-full bg-primary/60" />
      <div class="min-w-0 flex-1 bg-card border border-border rounded px-3 py-2 shadow-paper">
        <div class="text-xs font-medium mb-1 text-primary flex items-baseline gap-2">
          <span>{{ $t('session.you') }}</span>
          <span
            v-if="timeLabel"
            class="text-muted-foreground/60 font-normal tabular-nums"
          >{{ timeLabel }}</span>
        </div>
        <MsgClamp>
          <UserMsgContent :blocks="contentBlocks(group.user as any)" :record-uuid="group.user.uuid" />
        </MsgClamp>
      </div>
    </div>
  </div>
  <!-- 渠道切换横线:用户消息锚点 -->
  <DividerMark
    v-for="(m, j) in (group.user?.uuid ? channelMarksByUuid.get(group.user.uuid) ?? [] : [])"
    :key="`channel-mark-${group.user?.uuid}-${j}`"
    icon="i-carbon-cloud"
    :label="channelMarkLabel(m)"
  />
  <!-- 同一用户轮次的所有 assistant 记录共用一条回复轨道与一组头尾元信息。 -->
  <AssistantResponseFrame
    v-if="hasAssistantResponses"
    :meta="responseMeta"
    :show-footer="!!responseMeta"
  >
    <template v-for="entry in responseEntries" :key="entry.key">
      <div
        v-if="entry.grouped"
        v-memo="[entry]"
        class="assistant-response-entry"
        :class="{ 'response-cv': granularVisibility }"
      >
        <ToolProcessGroup :blocks="entry.blocks" :tools="toolsOf(entry.blocks)" />
      </div>
      <template v-else v-for="part in entry.parts" :key="part.record.uuid || part.index">
        <SystemEventRow v-if="part.record.type === 'system'" :record="part.record" />
        <div
          v-else
          v-memo="[part.record]"
          class="assistant-response-entry"
          :class="{ 'response-cv': granularVisibility }"
        >
          <ContentBlockList
            :blocks="part.blocks"
            :record-uuid="part.record.uuid"
          />
        </div>
      </template>
      <!-- 渠道切换横线:回复消息锚点 -->
      <template v-for="part in entry.parts" :key="`marks-${part.record.uuid || part.index}`">
        <DividerMark
          v-for="(m, j) in (part.record.uuid ? channelMarksByUuid.get(part.record.uuid) ?? [] : [])"
          :key="`channel-mark-${part.record.uuid}-${j}`"
          icon="i-carbon-cloud"
          :label="channelMarkLabel(m)"
        />
      </template>
    </template>
  </AssistantResponseFrame>
  <!-- 无 assistant 的系统轮保持原来的独立系统事件样式。 -->
  <template v-else v-for="entry in responseEntries" :key="entry.key">
    <template v-for="part in entry.parts" :key="part.record.uuid || part.index">
      <SystemEventRow v-if="part.record.type === 'system'" :record="part.record" />
    </template>
  </template>
</template>

<style scoped>
/* 吸顶层接管展示时文档流原件隐形占位 */
.user-msg-ghost { visibility: hidden; }
/* 末组不能整体虚拟化（吸顶/锚点需要完整容器），但其中已落账响应可逐条跳过
   屏外 style/layout/paint；auto 会在首次渲染后记住真实高度，降低滚动条漂移。 */
.response-cv {
  content-visibility: auto;
  contain-intrinsic-size: auto 220px;
}
</style>

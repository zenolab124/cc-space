<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionRecord, ContentBlock } from '@/types'
import type { ChannelMark } from '@/composables/useSessionSettings'
import { useToolDisplayMode, type ToolDisplayMode } from '@/composables/useToolDisplay'
import { segmentToolBlocks, type ToolUseBlock } from '@/utils/toolDisplay'
import type { AssistantResponseMeta } from '@/utils/assistantResponse'
import type { ConversationTurnView } from '@/engines/presentation'
import ContentBlockList from './ContentBlockList.vue'
import ToolProcessTrack from './ToolProcessTrack.vue'
import SystemEventRow from './SystemEventRow.vue'
import UserMsgContent from './UserMsgContent.vue'
import DividerMark from './DividerMark.vue'
import ConversationTurn from './session/ConversationTurn.vue'
import ArtifactPreviewList from './artifacts/ArtifactPreviewList.vue'
import { detectContentBlockArtifacts } from '@/features/artifact-preview/detectArtifacts'

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
  autoOpenArtifact?: boolean
}>()

const { t: _t } = useI18n() // 保持导入以便模板 $t 可用
const { toolDisplayModeFor } = useToolDisplayMode()
const turnDisplayMode = computed(() => toolDisplayModeFor('claude-code'))
const hasAssistantResponses = computed(() => props.group.responses.some(record => record.type === 'assistant'))
const normalUserVisible = computed(() => {
  const user = props.group.user
  return !!user
    && user.type === 'user'
    && !props.modelSwitchName(user)
    && !props.isModelCommandRecord(user)
    && !props.isSystemOnlyUser(user)
    && props.userHasVisibleContent(user)
})
const turnView = computed<ConversationTurnView>(() => ({
  dayLabel: props.dayLabel ?? null,
  timeLabel: props.timeLabel ?? null,
  user: {
    visible: normalUserVisible.value,
  },
  response: {
    visible: hasAssistantResponses.value,
    meta: props.responseMeta ?? null,
    showFooter: !!props.responseMeta,
    accent: 'claude',
  },
  lazy: false,
}))
const artifactRoot = computed(() => {
  const records = [props.group.user, ...props.group.responses]
  const record = records.find((candidate): candidate is Extract<VisibleRecord, { type: 'user' | 'assistant' }> =>
    candidate?.type === 'user' || candidate?.type === 'assistant')
  return record?.cwd ?? ''
})
const artifactCandidates = computed(() => detectContentBlockArtifacts(
  props.group.responses.flatMap(record => record.type === 'assistant' ? props.contentBlocks(record) : []),
))

interface ResponsePart {
  record: VisibleRecord
  index: number
  blocks: ContentBlock[]
  displayMode: ToolDisplayMode
}

interface ResponseEntry {
  key: string
  parts: ResponsePart[]
}

function toolsOf(blocks: ContentBlock[]): ToolUseBlock[] {
  return segmentToolBlocks(blocks).flatMap(segment => segment.kind === 'tools' ? segment.tools : [])
}

function withoutToolProcess(blocks: ContentBlock[]): ContentBlock[] {
  return segmentToolBlocks(blocks).flatMap(segment => segment.kind === 'block' ? [segment.block] : [])
}

const turnToolBlocks = computed(() => props.group.responses.flatMap(record =>
  record.type === 'assistant'
    ? segmentToolBlocks(props.contentBlocks(record)).flatMap(segment =>
        segment.kind === 'tools' ? segment.blocks : [])
    : []))
const turnTools = computed(() => toolsOf(turnToolBlocks.value))

const responseEntries = computed<ResponseEntry[]>(() => props.group.responses.map((record, index) => {
  const sourceBlocks = record.type === 'assistant' ? props.contentBlocks(record) : []
  const displayMode = toolDisplayModeFor('claude-code')
  const blocks = displayMode === 'grouped' ? withoutToolProcess(sourceBlocks) : sourceBlocks
  return {
    key: record.uuid ?? `${record.type}:${index}`,
    parts: [{ record, index, blocks, displayMode }],
  }
}))
</script>

<template>
  <ConversationTurn :turn="turnView">
    <template #before-user>
      <!-- /model 的 stdout 是执行成功的事实源；命令记录本身保持静默。 -->
      <DividerMark
        v-if="group.user && group.user.type === 'user' && modelSwitchName(group.user)"
        icon="i-carbon-model-alt"
        :label="$t('session.modelSwitchMark', { name: modelSwitchName(group.user) })"
      />
      <div v-else-if="group.user && group.user.type === 'user' && isSystemOnlyUser(group.user)" class="pl-3">
        <ContentBlockList
          :blocks="contentBlocks(group.user as any)"
          :record-uuid="group.user.uuid"
        />
      </div>
    </template>

    <template #user>
      <UserMsgContent
        v-if="group.user && group.user.type === 'user'"
        :blocks="contentBlocks(group.user as any)"
        :record-uuid="group.user.uuid"
      />
    </template>

    <template #after-user>
      <DividerMark
        v-for="(m, j) in (group.user?.uuid ? channelMarksByUuid.get(group.user.uuid) ?? [] : [])"
        :key="`channel-mark-${group.user?.uuid}-${j}`"
        icon="i-carbon-cloud"
        :label="channelMarkLabel(m)"
      />
    </template>

    <template #response>
      <ToolProcessTrack
        v-if="turnDisplayMode === 'grouped' && turnTools.length"
        :blocks="turnToolBlocks"
        :tools="turnTools"
      />
      <template v-for="entry in responseEntries" :key="entry.key">
        <template v-for="part in entry.parts" :key="part.record.uuid || part.index">
          <SystemEventRow v-if="part.record.type === 'system'" :record="part.record" />
          <div
            v-else-if="part.blocks.length"
            v-memo="[part.record]"
            class="assistant-response-entry"
            :class="{ 'response-cv': granularVisibility }"
          >
            <ContentBlockList
              :blocks="part.blocks"
              :record-uuid="part.record.uuid"
              :display-mode="part.displayMode"
            />
          </div>
        </template>
        <template v-for="part in entry.parts" :key="`marks-${part.record.uuid || part.index}`">
          <DividerMark
            v-for="(m, j) in (part.record.uuid ? channelMarksByUuid.get(part.record.uuid) ?? [] : [])"
            :key="`channel-mark-${part.record.uuid}-${j}`"
            icon="i-carbon-cloud"
            :label="channelMarkLabel(m)"
          />
        </template>
      </template>
      <ArtifactPreviewList
        :candidates="artifactCandidates"
        :root="artifactRoot"
        :auto-open="autoOpenArtifact"
      />
    </template>

    <template #without-response>
      <template v-for="entry in responseEntries" :key="entry.key">
        <template v-for="part in entry.parts" :key="part.record.uuid || part.index">
          <SystemEventRow v-if="part.record.type === 'system'" :record="part.record" />
        </template>
      </template>
    </template>
  </ConversationTurn>
</template>

<style scoped>
/* 末组不能整体虚拟化（吸顶/锚点需要完整容器），但其中已落账响应可逐条跳过
   屏外 style/layout/paint；auto 会在首次渲染后记住真实高度，降低滚动条漂移。 */
.response-cv {
  content-visibility: auto;
  contain-intrinsic-size: auto 220px;
}
</style>

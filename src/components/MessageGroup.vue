<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { SessionRecord, ContentBlock } from '@/types'
import type { ChannelMark } from '@/composables/useSessionSettings'
import { shortModel, formatTokens } from '@/types'
import MessageBlock from './MessageBlock.vue'
import SystemEventRow from './SystemEventRow.vue'
import MsgClamp from './MsgClamp.vue'
import UserMsgContent from './UserMsgContent.vue'

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
  footerAt?: number | null
  footer?: { text: string; doneFull: string } | null
  channelMarksByUuid: Map<string | null, ChannelMark[]>
  modelSwitchName: (record: any) => string | null
  isModelCommandRecord: (record: any) => boolean
  isSystemOnlyUser: (record: any) => boolean
  userHasVisibleContent: (record: any) => boolean
  contentBlocks: (record: any) => ContentBlock[]
  channelMarkLabel: (mark: ChannelMark) => string
}>()

const { t: _t } = useI18n() // 保持导入以便模板 $t 可用
</script>

<template>
  <!-- 跨天分隔:这轮起进入新的一天(首组标会话起始日) -->
  <div v-if="dayLabel" class="day-divider">
    <div class="day-line day-line-l" />
    <span class="day-text">{{ dayLabel }}</span>
    <div class="day-line day-line-r" />
  </div>
  <!-- 用户消息:有 AI 回复时吸顶,无回复的短轮次不启用(减少 sticky 元素数量) -->
  <!-- /model 切换成功(stdout 事实源):渲染成与渠道切换同款的配置分界横线 -->
  <div
    v-if="group.user && group.user.type === 'user' && modelSwitchName(group.user)"
    class="channel-mark"
  >
    <div class="flex-1 h-px bg-border" />
    <span class="i-carbon-model-alt w-3 h-3" />
    <span>{{ $t('session.modelSwitchMark', { name: modelSwitchName(group.user) }) }}</span>
    <div class="flex-1 h-px bg-border" />
  </div>
  <!-- /model 命令记录本身:静默(事件由上面的 stdout 横线承载;取消选择时无 stdout,不留痕) -->
  <template v-else-if="group.user && group.user.type === 'user' && isModelCommandRecord(group.user)" />
  <!-- 纯系统注入(无真实用户输入):降级为系统注解样式 -->
  <div v-else-if="group.user && group.user.type === 'user' && isSystemOnlyUser(group.user)" class="pl-3">
    <MessageBlock
      v-for="(block, bi) in contentBlocks(group.user as any)"
      :key="`${group.user.uuid}-${bi}-${block.type}`"
      :block="block"
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
  <div
    v-for="(m, j) in (group.user?.uuid ? channelMarksByUuid.get(group.user.uuid) ?? [] : [])"
    :key="`channel-mark-${group.user?.uuid}-${j}`"
    class="channel-mark"
  >
    <div class="flex-1 h-px bg-border" />
    <span class="i-carbon-cloud w-3 h-3" />
    <span>{{ channelMarkLabel(m) }}</span>
    <div class="flex-1 h-px bg-border" />
  </div>
  <!-- 回复(AI + system) -->
  <template v-for="(resp, ri) in group.responses" :key="resp.uuid || resp">
    <SystemEventRow v-if="resp.type === 'system'" :record="resp" />
    <div v-else class="flex gap-3 msg-block">
      <div class="w-0.5 shrink-0 rounded-full bg-claude/60" />
      <div class="min-w-0 flex-1">
        <div class="text-xs font-medium mb-1 text-claude flex items-center gap-1.5 flex-wrap">
          <span>
            {{ $t('session.claude') }}
            <span v-if="(resp as any).message?.model" class="text-muted-foreground font-normal">
              ({{ shortModel((resp as any).message.model) }})
            </span>
          </span>
          <span v-if="(resp as any).message?.usage" class="text-muted-foreground/70 font-normal tabular-nums">
            {{ formatTokens((resp as any).message.usage.input_tokens) }} in
            · {{ formatTokens((resp as any).message.usage.cache_read_input_tokens) }} cache
            · {{ formatTokens((resp as any).message.usage.cache_creation_input_tokens) }} new
            · {{ formatTokens((resp as any).message.usage.output_tokens) }} out
          </span>
        </div>
        <div>
          <MessageBlock
            v-for="(block, bi) in contentBlocks(resp as any)"
            :key="`${(resp as any).uuid}-${bi}-${block.type}`"
            :block="block"
            :record-uuid="(resp as any).uuid"
          />
        </div>
        <!-- 长轮次组末统计(全轮 usage 总和+完成时间):挂在最后一条有效 assistant 块内,与回复共用竖线 -->
        <div
          v-if="ri === footerAt && footer"
          class="mt-2 text-[11px] text-muted-foreground/70 tabular-nums w-fit"
          v-tooltip="footer.doneFull"
        >
          {{ footer.text }}
        </div>
      </div>
    </div>
    <!-- 渠道切换横线:回复消息锚点 -->
    <div
      v-for="(m, j) in (resp.uuid ? channelMarksByUuid.get(resp.uuid) ?? [] : [])"
      :key="`channel-mark-${resp.uuid}-${j}`"
      class="channel-mark"
    >
      <div class="flex-1 h-px bg-border" />
      <span class="i-carbon-cloud w-3 h-3" />
      <span>{{ channelMarkLabel(m) }}</span>
      <div class="flex-1 h-px bg-border" />
    </div>
  </template>
</template>

<style scoped>
/* 跨天分隔:两侧向中心渐显的细线 + 居中淡字,轻于消息内容不抢视线 */
.day-divider {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 8px 0 2px;
  font-size: 10px;
  user-select: none;
}
.day-text {
  color: color-mix(in oklch, var(--muted-foreground) 72%, transparent);
  letter-spacing: 0.04em;
  flex-shrink: 0;
}
.day-line { flex: 1; height: 1px; }
.day-line-l { background: linear-gradient(to right, transparent, var(--border)); }
.day-line-r { background: linear-gradient(to left, transparent, var(--border)); }

/* 吸顶层接管展示时文档流原件隐形占位 */
.user-msg-ghost { visibility: hidden; }
</style>

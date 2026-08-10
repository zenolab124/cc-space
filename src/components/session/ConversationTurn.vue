<script setup lang="ts">
import type { ConversationTurnView } from '@/engines/presentation'
import AssistantResponseFrame from '@/components/AssistantResponseFrame.vue'
import DividerMark from '@/components/DividerMark.vue'
import ConversationUserMessage from './ConversationUserMessage.vue'

defineProps<{
  turn: ConversationTurnView
}>()

</script>

<template>
  <section class="conversation-turn space-y-4" :class="turn.lazy && 'is-lazy'">
    <DividerMark v-if="turn.dayLabel" :label="turn.dayLabel" class="mt-2 mb-0.5" />
    <slot name="before-user" />

    <div v-if="turn.user.visible">
      <ConversationUserMessage :time-label="turn.timeLabel">
        <slot name="user" />
        <template #actions><slot name="user-actions" /></template>
      </ConversationUserMessage>
    </div>

    <slot name="after-user" />

    <AssistantResponseFrame
      v-if="turn.response.visible"
      :meta="turn.response.meta"
      :show-footer="turn.response.showFooter"
      :speaker="turn.response.speaker"
      :accent="turn.response.accent"
    >
      <slot name="response" />
    </AssistantResponseFrame>
    <slot v-else name="without-response" />
  </section>
</template>

<style scoped>
.conversation-turn.is-lazy {
  content-visibility: auto;
  contain-intrinsic-size: auto 280px;
}
</style>

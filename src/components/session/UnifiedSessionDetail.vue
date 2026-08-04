<script setup lang="ts">
import { computed } from 'vue'
import type { SessionSummary } from '@/types'
import { usesNativeSessionSurface } from '@/engines/integration'
import SessionDetail from '@/components/SessionDetail.vue'
import EngineSessionDetail from '@/components/engine/EngineSessionDetail.vue'

const props = withDefaults(defineProps<{
  session?: SessionSummary | null
  sessionId?: string | null
  mode?: 'archive' | 'workbench'
  hideInput?: boolean
}>(), {
  session: null,
  sessionId: null,
  mode: 'archive',
  hideInput: false,
})

const nativeSurface = computed(() =>
  !props.session?.engine || usesNativeSessionSurface(props.session.engine),
)
</script>

<template>
  <!-- 页面只面对这一个入口；这里选择数据/运行控制器，两条路径最终都进入 SessionSurface。 -->
  <SessionDetail
    v-if="nativeSurface"
    :mode="mode"
    :session-id="sessionId || session?.id"
    :hide-input="hideInput"
  />
  <EngineSessionDetail
    v-else-if="session"
    :session="session"
    :mode="mode"
    :hide-input="hideInput"
  />
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { resolveTool } from './blocks/tools'
import {
  isInteractiveTool,
  mcpServerOf,
  type PermissionRequest,
  type PermissionDecision,
} from '@/composables/usePermissionRequests'
import { getHint } from '@/composables/usePermissionHints'
import SessionApprovalCard, { type SessionApprovalOption } from '@/components/session/SessionApprovalCard.vue'

const { t } = useI18n()

const props = defineProps<{
  request: PermissionRequest
}>()

const emit = defineEmits<{
  (event: 'decide', decision: PermissionDecision): void
}>()

const ToolComponent = computed(() => resolveTool(props.request.toolName))
const isDanger = computed(() => props.request.danger !== null)
const showAllowSession = computed(() => !isInteractiveTool(props.request.toolName))
const mcpServer = computed(() =>
  isInteractiveTool(props.request.toolName) ? null : mcpServerOf(props.request.toolName),
)
const hint = computed(() => getHint(props.request.requestId))
const options = computed<SessionApprovalOption[]>(() => [
  {
    id: 'allow_once',
    label: t('permission.allowOnce'),
    tone: 'primary',
    icon: 'i-carbon-checkmark',
  },
  ...(showAllowSession.value ? [{
    id: 'allow_session',
    label: t('permission.allowSession'),
    tone: 'warn' as const,
    icon: 'i-carbon-time',
    title: t('permission.allowSessionHint'),
  }] : []),
  ...(mcpServer.value ? [{
    id: 'allow_server',
    label: t('permission.allowServer', { server: mcpServer.value }),
    tone: 'warn' as const,
    icon: 'i-carbon-plug',
    title: t('permission.allowServerHint', { server: mcpServer.value }),
  }] : []),
  {
    id: 'deny',
    label: t('common.deny'),
    tone: 'ghost',
    icon: 'i-carbon-close',
  },
])

function decide(decision: string) {
  emit('decide', decision as PermissionDecision)
}
</script>

<template>
  <SessionApprovalCard
    :title="t('permission.title')"
    :subject="request.toolName"
    :danger="isDanger"
    :danger-reason="request.danger?.reason"
    :options="options"
    default-option-id="allow_once"
    deny-option-id="deny"
    @decide="decide"
  >
    <template #hint>
      <div
        v-if="hint?.text || hint?.loading"
        class="mx-3 mt-1.5 flex items-start gap-1.5 rounded border border-border/60 bg-muted/40 px-2 py-1.5"
      >
        <span class="i-carbon-sparkle mt-px h-3.5 w-3.5 shrink-0 text-primary/60" aria-hidden="true" />
        <span v-if="hint.loading" class="text-[10px] italic text-muted-foreground">{{ t('permission.analyzing') }}</span>
        <span v-else class="text-[10px] leading-relaxed text-foreground/80">{{ hint.text }}</span>
      </div>
    </template>

    <component
      :is="ToolComponent"
      :input="request.input"
      :tool-use-id="request.requestId"
      :name="request.toolName"
    />
  </SessionApprovalCard>
</template>

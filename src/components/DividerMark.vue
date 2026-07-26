<script setup lang="ts">
// 会话流内的分界横线(跨天日期、渠道/模型切换共用):
// 两侧向中心渐显的细线 + 居中淡字,轻于消息内容不抢视线
defineProps<{
  label: string
  /** UnoCSS 图标 class,如 i-carbon-cloud;不传则纯文字 */
  icon?: string
}>()
</script>

<template>
  <div class="divider-mark">
    <div class="dm-line dm-line-l" />
    <span class="dm-content">
      <span v-if="icon" :class="icon" class="dm-icon" />
      <span>{{ label }}</span>
    </span>
    <div class="dm-line dm-line-r" />
  </div>
</template>

<style scoped>
.divider-mark {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 10px;
  user-select: none;
}
.dm-content {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
  color: color-mix(in oklch, var(--muted-foreground) 72%, transparent);
  letter-spacing: 0.04em;
}
.dm-icon {
  width: 12px;
  height: 12px;
  flex-shrink: 0;
}
.dm-line { flex: 1; height: 1px; }
.dm-line-l { background: linear-gradient(to right, transparent, var(--border)); }
.dm-line-r { background: linear-gradient(to left, transparent, var(--border)); }
</style>

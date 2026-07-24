<script setup lang="ts">
/**
 * RunnerDockList — 停靠面板列表视图
 * 分两段展示：运行中实例 + 候选命令行
 */
import { useI18n } from 'vue-i18n'
import type { RunnerSnapshot, RunnerStatus, RunnerCommand } from '@/types'

const props = defineProps<{
  runners: RunnerSnapshot[]
  commands: RunnerCommand[]
  selectedId: string | null
  projectName: string
}>()

const emit = defineEmits<{
  select: [id: string]
  stop: [id: string]
  restart: [id: string]
  launch: [commandId: string]
  removeCommand: [commandId: string]
}>()

const { t } = useI18n()

/** 判断是否处于活跃状态 */
function isActive(status: RunnerStatus): boolean {
  return status === 'running' || status === 'starting'
}

/** 状态圆点样式 */
function dotClass(status: RunnerStatus): string {
  switch (status) {
    case 'running': return 'dot-running'
    case 'starting': return 'dot-starting'
    case 'exited': return 'dot-exited'
    case 'killed': return 'dot-killed'
    case 'crashed':
    case 'spawn-failed': return 'dot-crashed'
    default: return ''
  }
}
</script>

<template>
  <div class="overflow-y-auto shrink-0">
    <!-- 运行中实例段（无实例时隐藏段头） -->
    <template v-if="runners.length > 0">
      <div class="sec-header">
        {{ t('runner.runningSection') }}<span class="sec-line" />
      </div>
      <div
        v-for="runner in runners"
        :key="runner.id"
        class="run-row"
        :class="{ selected: runner.id === selectedId }"
        @click="emit('select', runner.id)"
      >
        <span class="status-dot" :class="dotClass(runner.status)" />
        <span class="run-alias truncate">{{ runner.alias || runner.cmd }}</span>
        <span class="run-cmd">{{ runner.cmd }}</span>
        <!-- 运行时长（活跃态）或退出码 -->
        <span v-if="isActive(runner.status)" class="run-time">
          <slot name="duration" :runner="runner" />
        </span>
        <span v-else-if="runner.exitCode != null" class="exit-code">exit {{ runner.exitCode }}</span>
        <!-- 操作按钮 -->
        <button
          v-if="isActive(runner.status)"
          class="row-act row-act-stop"
          :title="t('runner.stop')"
          @click.stop="emit('stop', runner.id)"
        >&#x25A0;</button>
        <button
          v-else
          class="row-act row-act-play"
          :title="t('runner.restart')"
          @click.stop="emit('restart', runner.id)"
        >&#x21BB;</button>
      </div>
    </template>

    <!-- 候选命令段 -->
    <template v-if="commands.length > 0">
      <div class="sec-header">
        {{ t('runner.candidateSection') }} &middot; {{ projectName }}<span class="sec-line" />
      </div>
      <div
        v-for="cmd in commands"
        :key="cmd.id"
        class="sug-row group"
      >
        <span class="ghost-dot" />
        <span class="src-badge" :class="cmd.source === 'agent' ? 'src-agent' : 'src-user'">
          {{ cmd.source === 'agent' ? `&#x2726; ${t('runner.sourceAgent')}` : `&#x21BA; ${t('runner.sourceUser')}` }}
        </span>
        <span v-if="cmd.alias" class="sug-alias">{{ cmd.alias }}</span>
        <span class="sug-cmd">{{ cmd.cmd }}</span>
        <span v-if="cmd.note" class="sug-note" :title="cmd.note">{{ cmd.note }}</span>
        <button
          class="row-act row-act-play"
          :title="t('runner.launch')"
          @click.stop="emit('launch', cmd.id)"
        >&#x25B6;</button>
        <button
          class="row-act row-act-del"
          :title="t('runner.removeCommand')"
          @click.stop="emit('removeCommand', cmd.id)"
        >&#x2715;</button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.sec-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px 4px;
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.06em;
  color: var(--muted-foreground);
}
.sec-line {
  flex: 1;
  height: 1px;
  background: var(--border);
  opacity: 0.6;
}

.status-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
}
.dot-running {
  background: var(--run-running);
  box-shadow: 0 0 0 2.5px color-mix(in oklch, var(--run-running) 22%, transparent);
}
.dot-starting { background: var(--run-starting); }
.dot-exited { background: var(--run-exited); }
.dot-killed { background: var(--run-exited); }
.dot-crashed {
  background: var(--run-crashed);
  box-shadow: 0 0 0 2.5px color-mix(in oklch, var(--run-crashed) 22%, transparent);
}

.run-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  cursor: pointer;
  font-size: 12px;
}
.run-row:hover { background: var(--muted); }
.run-row.selected {
  background: var(--muted);
  box-shadow: inset 2px 0 0 var(--primary);
}
.run-alias { font-weight: 600; flex-shrink: 0; }
.run-cmd {
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--muted-foreground);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.run-time {
  font-size: 10.5px;
  color: var(--muted-foreground);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}
.exit-code {
  font-size: 10px;
  color: var(--run-crashed);
  font-family: var(--font-mono);
  flex-shrink: 0;
}
.row-act {
  width: 22px;
  height: 20px;
  border: none;
  background: none;
  border-radius: var(--radius);
  color: var(--muted-foreground);
  cursor: pointer;
  font-size: 10px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.row-act:hover { background: var(--secondary); color: var(--foreground); }
.row-act-stop:hover { color: var(--destructive); }
.row-act-play:hover { color: var(--primary); }

.sug-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 12px;
  font-size: 11.5px;
  color: var(--muted-foreground);
}
.sug-row:hover { background: var(--muted); color: var(--foreground); }
.ghost-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  border: 1.5px dashed var(--border);
  flex-shrink: 0;
}
.src-badge {
  font-size: 9px;
  padding: 1px 6px;
  border-radius: 6px;
  flex-shrink: 0;
  font-weight: 600;
  letter-spacing: 0.02em;
}
.src-agent {
  background: color-mix(in oklch, var(--primary) 12%, transparent);
  color: var(--primary);
}
.src-user {
  background: var(--muted);
  color: var(--muted-foreground);
}
.sug-alias { font-weight: 550; flex-shrink: 0; }
.sug-cmd {
  font-family: var(--font-mono);
  font-size: 10.5px;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  opacity: 0.85;
}
.sug-note {
  font-size: 10px;
  opacity: 0.7;
  flex-shrink: 0;
  max-width: 130px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row-act-del { opacity: 0; }
.sug-row:hover .row-act-del { opacity: 1; }
</style>

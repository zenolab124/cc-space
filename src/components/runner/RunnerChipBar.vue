<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { RunnerSnapshot, RunnerStatus, RunnerCommand } from '@/types'

const props = defineProps<{
  runners: RunnerSnapshot[]
  commands: RunnerCommand[]
  selectedId: string | null
}>()

const emit = defineEmits<{
  select: [id: string]
  stop: [id: string]
  restart: [id: string]
  launch: [commandId: string]
  removeCommand: [commandId: string]
}>()

const { t } = useI18n()

/** 状态点样式映射 */
function statusDotClass(status: RunnerStatus): string {
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

/** 运行中 chip 的 tooltip */
function chipTitle(runner: RunnerSnapshot): string {
  const parts = [runner.alias ? `${runner.alias} · ${runner.cmd}` : runner.cmd]
  if (runner.status === 'exited' && runner.exitCode != null) parts.push(`exit ${runner.exitCode}`)
  if (runner.status === 'crashed' && runner.exitCode != null) parts.push(`exit ${runner.exitCode}`)
  return parts.join(' · ')
}

/** 候选 chip 的 tooltip */
function ghostTitle(cmd: RunnerCommand): string {
  const parts = [cmd.alias ? `${cmd.alias} · ${cmd.cmd}` : cmd.cmd]
  if (cmd.note) parts.push(cmd.note)
  parts.push(t('runner.clickToLaunch'))
  return parts.join(' · ')
}
</script>

<template>
  <div class="flex items-center gap-1.5 flex-wrap px-3 pt-2 pb-1.5 shrink-0">
    <!-- 运行中的 chip -->
    <div
      v-for="runner in runners"
      :key="runner.id"
      class="chip group"
      :class="{ selected: runner.id === selectedId }"
    >
      <button
        type="button"
        class="chip-main"
        :title="chipTitle(runner)"
        @click="emit('select', runner.id)"
      >
        <span class="status-dot" :class="statusDotClass(runner.status)" />
        <span class="truncate max-w-72">{{ runner.cmd }}</span>
      </button>
      <!-- 悬停操作：运行中/启动中显示停止，崩溃/退出显示重启 -->
      <button
        v-if="runner.status === 'running' || runner.status === 'starting'"
        type="button"
        class="chip-act group-hover:inline-flex"
        :title="t('runner.stop')"
        :aria-label="t('runner.stop')"
        @click="emit('stop', runner.id)"
      >
        ■
      </button>
      <button
        v-else-if="runner.status === 'crashed' || runner.status === 'spawn-failed' || runner.status === 'exited'"
        type="button"
        class="chip-act chip-act-go group-hover:inline-flex"
        :title="t('runner.restart')"
        :aria-label="t('runner.restart')"
        @click="emit('restart', runner.id)"
      >
        ↻
      </button>
    </div>

    <!-- 分隔线（两侧都有内容时才显示） -->
    <div v-if="runners.length > 0 && commands.length > 0" class="chip-sep" />

    <!-- 候选命令 chip -->
    <div
      v-for="cmd in commands"
      :key="cmd.id"
      class="chip ghost group"
    >
      <button
        type="button"
        class="chip-main"
        :title="ghostTitle(cmd)"
        @click="emit('launch', cmd.id)"
      >
        <span class="src-icon" :class="cmd.source === 'agent' ? 'text-primary' : ''">
          {{ cmd.source === 'agent' ? '✦' : '↺' }}
        </span>
        <span class="truncate max-w-72">{{ cmd.cmd }}</span>
      </button>
      <button
        type="button"
        class="chip-act group-hover:inline-flex"
        :title="t('runner.removeCommand')"
        :aria-label="t('runner.removeCommand')"
        @click="emit('removeCommand', cmd.id)"
      >
        ✕
      </button>
    </div>
  </div>
</template>

<style scoped>
/* 状态点 */
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

/* Chip 基础样式 */
.chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  padding: 3px 9px;
  border-radius: var(--radius);
  cursor: pointer;
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--foreground);
  font-weight: 550;
  line-height: 1.5;
}
.chip:hover { box-shadow: var(--shadow-paper); }
.chip.selected {
  border-color: var(--primary);
  background: color-mix(in oklch, var(--primary) 9%, var(--card));
}
.chip.ghost {
  border-style: dashed;
  background: transparent;
  color: var(--muted-foreground);
  font-weight: 450;
}
.chip.ghost:hover {
  color: var(--foreground);
  background: var(--card);
}

.chip-main {
  display: inline-flex;
  align-items: center;
  min-width: 0;
  gap: 5px;
  padding: 0;
  border: none;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}

/* Chip 内操作按钮 */
.chip-act {
  display: none;
  border: none;
  background: none;
  cursor: pointer;
  font-size: 9px;
  padding: 0 1px;
  color: var(--muted-foreground);
  align-items: center;
}
.chip-act:hover { color: var(--destructive); }
.chip-act-go:hover { color: var(--primary); }

.src-icon { font-size: 9px; opacity: 0.8; }

/* 分隔竖线 */
.chip-sep {
  width: 1px;
  height: 16px;
  background: var(--border);
  margin: 0 3px;
  flex-shrink: 0;
}
</style>

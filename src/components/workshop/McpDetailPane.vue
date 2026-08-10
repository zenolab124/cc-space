<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { useConfirm } from '@/composables/useConfirm'
import { parentPath } from '@/utils/path'
import { showSystemOpenMenu } from '@/composables/useFileOpener'
import type { WorkshopMcpServer } from '@/types'

/**
 * MCP 服务器详情面板（v2.9.0 FR-004/FR-005）：
 * 展示选中 MCP 服务器的完整配置信息，提供移除/重置审批操作。
 * 写操作代理 claude mcp CLI，Monet 不直接修改配置文件。
 */

const props = defineProps<{
  server: WorkshopMcpServer | null
}>()

const emit = defineEmits<{
  (e: 'refresh'): void
}>()

const { t } = useI18n()
const { confirm } = useConfirm()

// 操作错误信息
const actionError = ref<string | null>(null)
const actionLoading = ref(false)

/** 判断是否由 settings.json 管理（不可通过 CLI 移除） */
function isManagedBySettings(server: WorkshopMcpServer): boolean {
  return server.source === '设置' || server.source === 'Settings'
}

/** 从 source 推导 scope 参数 */
function deriveScope(server: WorkshopMcpServer): string {
  if (server.source === '全局' || server.source === 'Global') return 'user'
  if (server.path.endsWith('.mcp.json')) return 'local'
  return 'project'
}

/** 从 path 推导 projectCwd */
function deriveProjectCwd(server: WorkshopMcpServer): string {
  // .mcp.json 文件的 projectCwd 是其所在目录（兼容 / 与 \ 分隔符）
  return parentPath(server.path) || server.path
}

/** 移除 MCP 服务器 */
async function handleRemove() {
  if (!props.server) return
  actionError.value = null

  const ok = await confirm(
    t('workshop.mcpRemoveConfirm', { name: props.server.name }),
    t('workshop.mcpRemove')
  )
  if (!ok) return

  actionLoading.value = true
  try {
    const scope = deriveScope(props.server)
    const projectCwd = scope !== 'user' ? deriveProjectCwd(props.server) : undefined
    await invoke('mcp_remove', {
      name: props.server.name,
      scope,
      projectCwd: projectCwd ?? null,
    })
    emit('refresh')
  } catch (e) {
    actionError.value = String(e)
  } finally {
    actionLoading.value = false
  }
}

/** 重置项目审批 */
async function handleResetChoices() {
  if (!props.server) return
  actionError.value = null
  actionLoading.value = true
  try {
    await invoke('mcp_reset_project_choices', {
      projectCwd: deriveProjectCwd(props.server),
    })
    emit('refresh')
  } catch (e) {
    actionError.value = String(e)
  } finally {
    actionLoading.value = false
  }
}

/** 打开来源文件 */
async function openSourceFile(systemDefault = false) {
  if (!props.server) return
  try {
    await invoke('open_asset_file', { path: props.server.path, systemDefault })
  } catch (_) {
    // 静默
  }
}

function showSourceFileMenu(event: MouseEvent) {
  const path = props.server?.path
  if (!path) return
  return showSystemOpenMenu(event, () => openSourceFile(true), path)
}

function dismissError() {
  actionError.value = null
}
</script>

<template>
  <!-- 空态：未选中 -->
  <div v-if="!server" class="detail-empty">
    <span class="text-xs text-muted-foreground">{{ t('workshop.selectToView') }}</span>
  </div>

  <!-- 正常态 -->
  <div v-else class="detail-content">
    <!-- 头部 -->
    <div class="detail-head">
      <h1 class="detail-title">
        <span>{{ server.name }}</span>
        <span
          class="status-badge"
          :class="{
            'badge-enabled': server.status === 'enabled',
            'badge-disabled': server.status === 'disabled',
            'badge-pending': server.status === 'pending',
          }"
        >
          {{ server.status === 'enabled' ? t('workshop.mcpEnabled') : server.status === 'disabled' ? t('workshop.mcpDisabled') : t('workshop.mcpPending') }}
        </span>
        <span class="detail-source-badge">{{ server.source }}</span>
      </h1>
      <div class="detail-path" @click="openSourceFile()" @contextmenu="showSourceFileMenu">{{ server.path }}</div>
    </div>

    <!-- 配置详情卡 -->
    <div class="fm-card">
      <div class="fm-title">{{ t('workshop.mcpDetail') }}</div>

      <!-- transport -->
      <div class="fm-row">
        <span class="fm-key">transport</span>
        <span class="fm-val mono">{{ server.transport }}</span>
      </div>

      <!-- endpoint -->
      <div class="fm-row">
        <span class="fm-key">endpoint</span>
        <span class="fm-val mono">{{ server.endpoint }}</span>
      </div>

      <!-- status -->
      <div class="fm-row">
        <span class="fm-key">status</span>
        <span class="fm-val">
          <span
            class="status-badge-inline"
            :class="{
              'badge-enabled': server.status === 'enabled',
              'badge-disabled': server.status === 'disabled',
              'badge-pending': server.status === 'pending',
            }"
          >{{ server.status }}</span>
          <span v-if="server.status === 'pending'" class="pending-note">
            {{ t('workshop.mcpPendingNote') }}
          </span>
        </span>
      </div>

      <!-- args -->
      <div class="fm-row">
        <span class="fm-key">args</span>
        <span class="fm-val">
          <template v-if="server.args.length > 0">
            <code v-for="(arg, i) in server.args" :key="i" class="arg-item">{{ arg }}</code>
          </template>
          <span v-else class="text-muted-foreground">{{ t('workshop.mcpNoArgs') }}</span>
        </span>
      </div>

      <!-- env keys（仅非空时展示） -->
      <div v-if="server.envKeys.length > 0" class="fm-row">
        <span class="fm-key">env</span>
        <span class="fm-val">
          <code v-for="(key, i) in server.envKeys" :key="i" class="arg-item">{{ key }}</code>
          <span class="env-note">{{ t('workshop.mcpEnvNote') }}</span>
        </span>
      </div>

      <!-- header keys（仅非空时展示） -->
      <div v-if="server.headerKeys.length > 0" class="fm-row">
        <span class="fm-key">headers</span>
        <span class="fm-val">
          <code v-for="(key, i) in server.headerKeys" :key="i" class="arg-item">{{ key }}</code>
          <span class="env-note">{{ t('workshop.mcpEnvNote') }}</span>
        </span>
      </div>

      <!-- 来源文件 -->
      <div class="fm-row">
        <span class="fm-key">{{ t('workshop.mcpSourceFile') }}</span>
        <span class="fm-val mono path-val" @click="openSourceFile()" @contextmenu="showSourceFileMenu">{{ server.path }}</span>
      </div>
    </div>

    <!-- 操作区 -->
    <div class="action-area">
      <template v-if="isManagedBySettings(server)">
        <p class="action-info">{{ t('workshop.mcpManagedBySettings') }}</p>
      </template>
      <template v-else>
        <button
          class="ws-btn btn-danger"
          :disabled="actionLoading"
          @click="handleRemove"
        >
          <span class="i-carbon-trash-can w-3 h-3" />
          {{ t('workshop.mcpRemove') }}
        </button>
        <button
          v-if="server.path.endsWith('.mcp.json')"
          class="ws-btn"
          :disabled="actionLoading"
          @click="handleResetChoices"
        >
          {{ t('workshop.mcpResetChoices') }}
        </button>
      </template>
    </div>

    <!-- 操作错误展示 -->
    <div v-if="actionError" class="error-bar">
      <span class="error-label">{{ t('workshop.mcpActionFailed') }}:</span>
      <span class="error-msg">{{ actionError }}</span>
      <button class="error-dismiss" @click="dismissError">×</button>
    </div>

    <!-- 底部说明 -->
    <p class="cli-note">{{ t('workshop.mcpCliNote') }}</p>
  </div>
</template>

<style scoped>
.detail-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  min-height: 200px;
}
.detail-content {
  padding: 20px 26px;
}
.detail-head {
  margin-bottom: 16px;
}
.detail-title {
  font-size: 16px;
  font-weight: 700;
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.detail-source-badge {
  font-size: 10px;
  border: 1px solid var(--border);
  border-radius: 3px;
  padding: 0 5px;
  color: var(--muted-foreground);
}
.detail-path {
  font-size: 10.5px;
  font-family: var(--font-mono);
  color: var(--muted-foreground);
  margin-top: 4px;
  word-break: break-all;
  cursor: pointer;
}
.detail-path:hover {
  text-decoration: underline;
}

/* 状态徽章 */
.status-badge,
.status-badge-inline {
  font-size: 10px;
  font-weight: 500;
  border-radius: 3px;
  padding: 1px 5px;
  white-space: nowrap;
}
.badge-enabled {
  background: oklch(0.92 0.03 145);
  color: var(--primary);
}
.badge-disabled {
  background: var(--muted);
  color: var(--muted-foreground);
}
.badge-pending {
  background: var(--amber-bg, oklch(0.92 0.04 70));
  color: var(--amber, oklch(0.62 0.14 70));
}
.pending-note {
  display: block;
  margin-top: 4px;
  font-size: 10.5px;
  color: var(--amber, oklch(0.62 0.14 70));
}

/* 配置卡 */
.fm-card {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  box-shadow: var(--shadow-paper);
  overflow: hidden;
  margin-bottom: 16px;
}
.fm-title {
  font-size: 11px;
  font-weight: 600;
  padding: 8px 12px;
  background: var(--secondary);
  border-bottom: 1px solid var(--border);
  color: var(--muted-foreground);
}
.fm-row {
  display: flex;
  padding: 6px 12px;
  border-bottom: 1px solid var(--border);
  font-size: 11.5px;
  gap: 8px;
  align-items: baseline;
}
.fm-row:last-child {
  border-bottom: none;
}
.fm-key {
  width: 90px;
  flex-shrink: 0;
  color: var(--muted-foreground);
  font-family: var(--font-mono);
  font-size: 10.5px;
}
.fm-val {
  flex: 1;
  min-width: 0;
  word-break: break-word;
}
.fm-val.mono {
  font-family: var(--font-mono);
  font-size: 11px;
}
.path-val {
  cursor: pointer;
}
.path-val:hover {
  text-decoration: underline;
}

/* args/env 行内标签 */
.arg-item {
  display: inline-block;
  font-family: var(--font-mono);
  font-size: 10.5px;
  background: var(--muted);
  border-radius: 3px;
  padding: 1px 5px;
  margin: 1px 4px 1px 0;
}
.env-note {
  font-size: 10px;
  color: var(--muted-foreground);
  margin-left: 4px;
}

/* 操作区 */
.action-area {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}
.action-info {
  font-size: 11px;
  color: var(--muted-foreground);
}
.ws-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  padding: 3px 12px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--card);
  cursor: pointer;
}
.ws-btn:hover {
  box-shadow: var(--shadow-paper);
}
.ws-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn-danger {
  color: var(--destructive);
  border-color: color-mix(in oklch, var(--destructive) 40%, transparent);
}
.btn-danger:hover {
  background: color-mix(in oklch, var(--destructive) 8%, var(--card));
}

/* 错误条 */
.error-bar {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 8px 10px;
  margin-bottom: 12px;
  border-radius: var(--radius);
  background: color-mix(in oklch, var(--destructive) 8%, var(--card));
  border: 1px solid color-mix(in oklch, var(--destructive) 30%, transparent);
  font-size: 11px;
}
.error-label {
  color: var(--destructive);
  font-weight: 600;
  flex-shrink: 0;
}
.error-msg {
  flex: 1;
  min-width: 0;
  word-break: break-word;
  color: var(--foreground);
}
.error-dismiss {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: none;
  cursor: pointer;
  color: var(--muted-foreground);
  font-size: 14px;
  line-height: 1;
  border-radius: 3px;
}
.error-dismiss:hover {
  background: var(--muted);
}

/* 底部说明 */
.cli-note {
  font-size: 10.5px;
  color: var(--muted-foreground);
  padding-top: 4px;
}
</style>

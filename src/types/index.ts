import i18n from '../locales'
import type { EngineInstanceId, ProjectRef, SessionRef } from '../engines/types'

// 与 Rust 端数据模型对应的 TypeScript 类型

export interface TokenUsage {
  input_tokens: number
  output_tokens: number
  cache_creation_input_tokens: number
  cache_read_input_tokens: number
}

export function hasReportedUsage(usage: Partial<TokenUsage> | null | undefined): boolean {
  if (!usage) return false
  return (usage.input_tokens ?? 0)
    + (usage.output_tokens ?? 0)
    + (usage.cache_creation_input_tokens ?? 0)
    + (usage.cache_read_input_tokens ?? 0) > 0
}

export function shouldReplaceUsage(
  current: Partial<TokenUsage> | null | undefined,
  currentStopReason: string | null | undefined,
  candidate: Partial<TokenUsage> | null | undefined,
  candidateStopReason: string | null | undefined,
): boolean {
  const currentReported = hasReportedUsage(current)
  const candidateReported = hasReportedUsage(candidate)
  if (candidateReported !== currentReported) return candidateReported
  if (!candidateReported) return false
  const currentTerminal = !!currentStopReason
  const candidateTerminal = !!candidateStopReason
  if (candidateTerminal !== currentTerminal) return candidateTerminal
  return true
}

export interface WorkspaceContext {
  kind: 'primary' | 'linked' | 'legacy'
  worktreeRoot: string
  mainRoot: string | null
  name: string | null
  branch: string | null
  available: boolean
  mainAvailable: boolean
  detectedBy: 'git' | 'persisted' | 'convention'
}

export interface SessionSummary {
  id: string
  title: string | null
  first_user_message: string | null
  model: string | null
  git_branch: string | null
  cwd: string | null
  version: string | null
  timestamp: string | null
  last_modified: number
  /** 会话总消耗（已含子 Agent/工作流） */
  total_tokens: TokenUsage
  /** 其中子 Agent/工作流分项（total_tokens 已含） */
  subagent_tokens: TokenUsage
  file_size: number
  message_count: number
  context_window: number | null
  /** 多引擎结构化身份；旧草稿会话在落盘前可缺省。 */
  reference?: SessionRef
  project_reference?: ProjectRef
  engine?: EngineInstanceId
  engine_name?: string
  native_id?: string
  /** 引擎原生摘要元数据（例如 Codex modelProvider）；只读透传。 */
  source_meta?: Record<string, unknown>
  /** Monet 派生的 Git workspace 关系；不改变原始会话身份与 cwd。 */
  workspace_context?: WorkspaceContext
}

export interface Project {
  id: string
  display_path: string
  /** 引擎确认过的真实工作目录；仅用于创建会话，不能由展示名称推断。 */
  source_path?: string | null
  sessions: SessionSummary[]
  session_count: number
  last_active: number | null
  reference?: ProjectRef
  engine?: EngineInstanceId
  engine_name?: string
  native_id?: string
}

// --- 会话记录（完整对话）---

export interface UserMessage {
  role: string | null
  content: string | ContentBlock[]
}

export interface AssistantMessage {
  id: string | null
  message_type: string | null
  role: string | null
  content: ContentBlock[]
  model: string | null
  stop_reason: string | null
  usage: TokenUsage | null
}

// serde(tag = "type", rename_all = "snake_case") — 与 Rust 序列化对齐
export type ContentBlock =
  | { type: 'text'; text: string }
  | { type: 'thinking'; thinking: string; signature?: string; _thinkingMs?: number }
  | { type: 'redacted_thinking'; [key: string]: unknown }
  | { type: 'tool_use'; id: string; name: string; input: Record<string, unknown>; _title?: string; _presentation?: 'orchestration' }
  | { type: 'tool_result'; tool_use_id: string; content: string | ContentBlock[]; is_error: boolean }
  // img_index: record 内第 N 个 image block(0 起,深度优先),Rust 解析时注入,前端拼 ccimg 协议 URL 用
  // data: 仅 pending/流式区(JSONL 未落盘)的内存路径存在;历史区已落盘,走协议 URL,data 缺省
  | { type: 'image'; source: { source_type: string; media_type: string; img_index: number; data?: string } }
  | { type: 'document'; source: { source_type: string; media_type: string }; title: string | null }
  | { type: string; [key: string]: unknown }

/** 顶层 toolUseResult 的异步任务精简提取（Rust AsyncMeta，字段全可缺省） */
export interface AsyncMeta {
  background_task_id: string | null
  status: string | null
  is_async: boolean | null
  agent_id: string | null
  agent_type: string | null
  resolved_model: string | null
  description: string | null
  task_id: string | null
  task_type: string | null
  workflow_name: string | null
  run_id: string | null
  summary: string | null
  output_file: string | null
  scheduled_for: number | null
  timeout_ms: number | null
  persistent: boolean | null
  resumed_agent_id: string | null
}

export type SessionRecord =
  | { type: 'user'; uuid: string | null; parent_uuid: string | null; session_id: string | null; timestamp: string | null; cwd: string | null; version: string | null; git_branch: string | null; is_sidechain: boolean | null; message: UserMessage | null; async_meta: AsyncMeta | null; origin_kind: string | null }
  | { type: 'assistant'; uuid: string | null; parent_uuid: string | null; session_id: string | null; timestamp: string | null; cwd: string | null; version: string | null; git_branch: string | null; is_sidechain: boolean | null; message: AssistantMessage | null }
  // system 记录字段随 Rust SystemRecord 的 rename_all = "camelCase" 序列化
  | { type: 'system'; subtype: string | null; content: string | null; level: string | null; timestamp: string | null; uuid: string | null; error: Record<string, unknown> | null; compactMetadata: Record<string, unknown> | null; retryAttempt: number | null; maxRetries: number | null }
  | { type: 'task_notification'; timestamp: string | null; session_id: string | null; content: string }
  | { type: 'ai_title'; session_id: string | null; ai_title: string }
  | { type: 'queue_operation'; operation: string | null; timestamp: string | null; session_id: string | null }
  | { type: 'file_history_snapshot'; message_id: string | null; is_snapshot_update: boolean | null }
  | { type: 'unknown'; raw_type: string }

// --- 首页统计（v2.2.0，与 Rust usage_stats / probe 模块对应）---

export interface DailyUsage {
  date: string
  total: number
}

export interface ModelUsage {
  model: string
  total: number
}

export interface UsageStats {
  total: number
  daily: DailyUsage[]
  month: {
    total: number
    byModel: ModelUsage[]
  }
  byEngine: Array<{
    engineId: string
    total: number
    daily: DailyUsage[]
    month: {
      total: number
      byModel: ModelUsage[]
    }
  }>
}

/** get_schema_diagnosis 返回的子集——前端只消费诊断卡需要的字段 */
export interface SchemaDiagnosis {
  scanned_files: number
  record_types: {
    supported: Record<string, number>
    unknown: Record<string, unknown>
  }
  tools: {
    generic_undeclared: Record<string, unknown>
  }
}

// --- 工坊域(v2.3.0):get_workshop_assets 返回结构,Rust 端 #[serde(rename_all = "camelCase")] ---

export interface WorkshopSkill {
  name: string
  description: string
  argumentHint: string | null
  version: string | null
  source: string
  scope?: 'project' | 'repo' | 'user' | 'admin' | 'system' | string
  path: string
}

export interface WorkshopCommand {
  name: string
  description: string
  argumentHint: string | null
  source: string
  scope?: 'project' | 'repo' | 'user' | 'admin' | 'system' | string
  path: string
}

export interface WorkshopAgent {
  name: string
  description: string
  source: string
  path: string
}

// --- 子 Agent 元数据(list_subagents 返回) ---

export interface SubAgentMeta {
  agent_id: string
  tool_use_id: string
  agent_type: string | null
  description: string | null
  workflow_id?: string | null
}

/** 常见三值（PRD 口径）；Rust 端对配置显式 type 原样透传，运行时可能出现其他字符串 */
export type McpTransport = 'stdio' | 'http' | 'sse' | (string & {})

/** MCP 服务器三态（v2.9.0 FR-003）：启用 / 已禁用 / 待批准 */
export type McpServerStatus = 'enabled' | 'disabled' | 'pending'

export interface WorkshopMcpServer {
  name: string
  transport: McpTransport
  endpoint: string
  /** @deprecated 使用 status 三态代替（v2.9.0 FR-003） */
  enabled: boolean
  status: McpServerStatus
  args: string[]
  envKeys: string[]
  headerKeys: string[]
  source: string
  path: string
}

export interface WorkshopAssets {
  skills: WorkshopSkill[]
  commands: WorkshopCommand[]
  agents: WorkshopAgent[]
  mcpServers: WorkshopMcpServer[]
}

// --- 资产详情（v2.9.0 FR-001）---

export interface AssetDetail {
  frontmatter: Record<string, unknown> | null
  body: string
  mtime: number
  sizeBytes: number
  truncated?: boolean
}

// --- Hooks 聚合陈列（v2.9.0 FR-005）---

export type HookType = 'command' | 'prompt' | 'agent' | 'http' | 'mcp_tool'

export interface HookEntry {
  matcher: string | null
  hookType: HookType
  summary: string
  sourceLayer: string
  sourceFile: string
  config: Record<string, unknown>
}

export interface HookEventGroup {
  event: string
  entries: HookEntry[]
}

export interface HooksOverview {
  events: HookEventGroup[]
  parseFailures: string[]
}

// --- 记忆数据层（v2.9.0 FR-006）---

export type MemoryType = 'user' | 'feedback' | 'project' | 'reference' | 'unknown'

export interface MemoryEntry {
  file: string
  name: string
  description: string
  type: MemoryType
  mtime: number
  sizeBytes: number
  wikiLinks: string[]
  indexed: boolean
}

export interface MemoryProject {
  projectDir: string
  displayName: string
  count: number
  totalBytes: number
  lastModified: number
  entries: MemoryEntry[]
  legacyIndex?: boolean
  /** 悬空引用：MEMORY.md 索引了但磁盘不存在的文件名（体检数据源） */
  danglingRefs: string[]
}

export interface MemoryOverview {
  projects: MemoryProject[]
}

export interface MemoryDetail {
  frontmatter: Record<string, unknown> | null
  body: string
  mtime: number
  sizeBytes: number
}

// --- Runner 域（v2.11.0）---

/** 跑单进程状态 */
export type RunnerStatus = 'starting' | 'running' | 'exited' | 'killed' | 'crashed' | 'spawn-failed'

/** 跑单进程快照（Rust serde camelCase） */
export interface RunnerSnapshot {
  id: string
  sessionId: string
  alias: string | null
  cmd: string
  cwd: string
  status: RunnerStatus
  startedAt: number
  exitedAt: number | null
  exitCode: number | null
  pid: number | null
  logPath: string
}

/** 日志行 */
export interface LogLine {
  seq: number
  ts: number
  stream: 'stdout' | 'stderr'
  text: string
}

/** 候选命令（项目级资产） */
export interface RunnerCommand {
  id: string
  cmd: string
  cwd: string | null
  alias: string | null
  note: string | null
  source: 'agent' | 'user'
  createdAt: number
  lastUsedAt: number
}

// --- 工具函数 ---

export function tokenTotal(t: TokenUsage): number {
  return t.input_tokens + t.output_tokens + t.cache_creation_input_tokens + t.cache_read_input_tokens
}

export function formatTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)}B`
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${bytes} B`
}

export function relativeTime(timestamp: string | number): string {
  const date = typeof timestamp === 'number'
    ? new Date(timestamp * 1000)
    : new Date(timestamp)
  const now = Date.now()
  const diff = now - date.getTime()
  const seconds = Math.floor(diff / 1000)
  const minutes = Math.floor(seconds / 60)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)

  if (seconds < 60) return i18n.global.t('time.justNow')
  if (minutes < 60) return i18n.global.t('time.minutesAgo', { n: minutes })
  if (hours < 24) return i18n.global.t('time.hoursAgo', { n: hours })
  if (days === 1) return i18n.global.t('time.yesterday')
  if (days < 30) return i18n.global.t('time.daysAgo', { n: days })
  return date.toLocaleDateString('zh-CN')
}

export function shortModel(model: string | null): string {
  if (!model) return ''
  // CLI 本地合成的占位消息(API Error 等),非真实模型响应
  if (model === '<synthetic>') return i18n.global.t('session.system')
  const ver = model.match(/(\d+)[.-](\d+)/) ?? model.match(/[- ](\d+)(?:\b|$)/)
  const v = ver ? (ver[2] ? ` ${ver[1]}.${ver[2]}` : ` ${ver[1]}`) : ''
  if (model.includes('fable')) return 'Fable' + v
  if (model.includes('opus')) return 'Opus' + v
  if (model.includes('sonnet')) return 'Sonnet' + v
  if (model.includes('haiku')) return 'Haiku' + v
  return model
}

/** 会话显示标题（metaTitle 优先于 JSONL 原始标题） */
export function displayTitle(s: SessionSummary, metaTitle?: string): string {
  if (metaTitle) return metaTitle
  if (s.title) return s.title
  if (s.first_user_message) return s.first_user_message
  return i18n.global.t('session.noTitleSession')
}

/** 短 UUID（前 8 位）*/
export function shortId(id: string): string {
  return id.slice(0, 8)
}

export interface EngineInstanceId {
  engineId: string
  instanceId: string
}

export interface ProjectRef {
  engine: EngineInstanceId
  nativeId: string
}

export interface SessionRef {
  engine: EngineInstanceId
  nativeId: string
}

export interface EngineCapabilities {
  history: {
    pagination: 'native' | 'emulated' | 'none'
    changeDelivery: 'push' | 'watch' | 'poll' | 'none'
    search: boolean
    assets: boolean
  }
  runtime: null | {
    create: boolean
    resume: boolean
    fork: boolean
    steer: boolean
    interrupt: boolean
    streaming: {
      text: 'delta' | 'item' | 'final' | 'none'
      reasoning: 'delta' | 'item' | 'final' | 'none'
      toolProgress: 'delta' | 'item' | 'final' | 'none'
    }
    modelCatalog: 'dynamic' | 'configured' | 'none'
    interactions: string[]
  }
  facets: {
    assets: boolean
    automation: boolean
    configuration: boolean
    quota: boolean
    runtimeCommands: boolean
  }
}

export interface EngineDescriptor {
  instance: EngineInstanceId
  displayName: string
  enabled: boolean
  capabilities: EngineCapabilities
  ui: {
    identity: 'structured' | 'native'
    sessionSurface: 'standard' | 'native'
    installGuideUrl: string | null
    configurationGuideUrl: string | null
  }
}

export interface FacetItem {
  id: string
  displayName: string
  description: string | null
  data: unknown
}

export interface EngineHealth {
  instance: EngineInstanceId
  status: 'available' | 'degraded' | 'unavailable' | 'disabled'
  installed: boolean
  authenticated: boolean | null
  version: string | null
  versionSupported: boolean | null
  executablePath: string | null
  source: { available: boolean; reasonCode: string | null }
  runtime: { available: boolean; reasonCode: string | null }
  diagnostics: Array<{ code: string; message: string }>
}

export interface EngineProject {
  reference: ProjectRef
  displayName: string
  displayPath: string | null
  sessionCount: number
  lastActive: string | null
}

export interface EngineUsage {
  inputTokens: number
  outputTokens: number
  totalTokens: number | null
  cachedInputTokens: number | null
  cacheCreationInputTokens: number | null
}

export interface EngineSessionSummary {
  reference: SessionRef
  project: ProjectRef
  title: string | null
  preview: string | null
  cwd: string | null
  model: string | null
  createdAt: string | null
  updatedAt: string | null
  usage: EngineUsage | null
  sourceMeta: Record<string, unknown>
}

export interface SessionActions {
  resume: { available: boolean; reasonCode: string | null }
  fork: { available: boolean; reasonCode: string | null }
  send: { available: boolean; reasonCode: string | null }
  steer: { available: boolean; reasonCode: string | null }
  interrupt: { available: boolean; reasonCode: string | null }
  openCwd: { available: boolean; reasonCode: string | null }
}

export type EngineSegment =
  | { kind: 'text'; text: string }
  | { kind: 'reasoning'; text: string; visibility: 'visible' | 'summary' | 'redacted' }
  | { kind: 'toolCall'; id: string; name: string; input: unknown }
  | { kind: 'toolResult'; callId: string; content: unknown; isError: boolean }
  | { kind: 'commandExecution'; id: string; command: string; cwd: string | null; output: string | null; status: string }
  | { kind: 'fileChange'; id: string; changes: Array<{ path: string; kind: string; diff: string | null }>; status: string }
  | { kind: 'attachment'; asset: { session: SessionRef; nativeId: string }; mediaType: string; title: string | null }
  | { kind: 'unknown'; typeName: string; summary: string | null }

export interface ConversationRecord {
  id: string
  session: SessionRef
  turnId: string | null
  parentId: string | null
  role: 'user' | 'assistant' | 'system' | 'tool' | 'unknown'
  timestamp: string | null
  segments: EngineSegment[]
  usage: EngineUsage | null
  sourceMeta: Record<string, unknown>
}

export interface ConversationPage {
  records: ConversationRecord[]
  nextCursor: string | null
}

export interface InteractionRef {
  session: SessionRef
  runtimeId: { 0: string } | string
  requestId: string
  turnId: string | null
}

export interface InteractionRequest {
  reference: InteractionRef
  kind: string
  title: string | null
  payload: unknown
  options: Array<{ id: string; label: string; dangerous: boolean }>
}

export interface RuntimeSnapshot {
  session: SessionRef
  runtimeId: { 0: string } | string
  generation: number
  lastSequence: number
  sequenceConsistent: boolean
  phase: 'detached' | 'connecting' | 'idle' | 'running' | 'awaitingInteraction' | 'failed' | 'exited'
  activeTurnId: string | null
  pendingInteractions: InteractionRequest[]
  lastError: string | null
}

export interface RuntimeEventEnvelope {
  session: SessionRef
  runtimeId: { 0: string } | string
  generation: number
  sequence: number
  timestamp: string
  event: ({ kind: string } & Record<string, unknown>)
}

export interface ModelDescriptor {
  id: string
  model: string
  displayName: string
  description: string | null
  isDefault: boolean
  hidden: boolean
  defaultEffort: string | null
  efforts: Array<{ id: string; description: string | null }>
}

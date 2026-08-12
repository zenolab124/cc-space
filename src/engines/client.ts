import { invoke } from '@tauri-apps/api/core'
import type {
  ConversationPage,
  EngineDescriptor,
  EngineHealth,
  EngineInstanceId,
  EngineProject,
  EngineSegment,
  EngineSessionSummary,
  FacetItem,
  InteractionRef,
  ModelDescriptor,
  ProjectRef,
  SessionRef,
  SessionActions,
  RuntimeSnapshot,
  RuntimeInputItem,
  TurnHandle,
} from './types'
import { configureUiIntegrations } from './integration'

interface Page<T> {
  projects?: T[]
  sessions?: T[]
  nextCursor: string | null
}

const PAGE_LIMIT = 200
const MAX_PAGES = 10_000

export async function listEngines(): Promise<EngineDescriptor[]> {
  const result = await invoke<{ engines: EngineDescriptor[] }>('engine_list')
  configureUiIntegrations(result.engines)
  return result.engines
}

export function engineHealth(instance: EngineInstanceId): Promise<EngineHealth> {
  return invoke('engine_health', { instance })
}

export function setEngineEnabled(instance: EngineInstanceId, enabled: boolean): Promise<void> {
  return invoke('engine_set_enabled', { instance, enabled })
}

export async function listProjects(instance: EngineInstanceId): Promise<EngineProject[]> {
  const result: EngineProject[] = []
  let cursor: string | null = null
  for (let pageNumber = 0; pageNumber < MAX_PAGES; pageNumber++) {
    const page: Page<EngineProject> = await invoke<Page<EngineProject>>('engine_list_projects', {
      instance,
      query: { cursor, limit: PAGE_LIMIT },
    })
    result.push(...(page.projects ?? []))
    if (!page.nextCursor || page.nextCursor === cursor) return result
    cursor = page.nextCursor
  }
  throw new Error('Project pagination exceeded the safety limit')
}

export async function listSessions(project: ProjectRef): Promise<EngineSessionSummary[]> {
  const result: EngineSessionSummary[] = []
  let cursor: string | null = null
  for (let pageNumber = 0; pageNumber < MAX_PAGES; pageNumber++) {
    const page: Page<EngineSessionSummary> = await invoke<Page<EngineSessionSummary>>('engine_list_sessions', {
      project,
      query: { cursor, limit: PAGE_LIMIT },
    })
    result.push(...(page.sessions ?? []))
    if (!page.nextCursor || page.nextCursor === cursor) return result
    cursor = page.nextCursor
  }
  throw new Error('Session pagination exceeded the safety limit')
}

export async function loadTimeline(session: SessionRef): Promise<ConversationPage> {
  const records: ConversationPage['records'] = []
  let cursor: string | null = null
  for (let pageNumber = 0; pageNumber < MAX_PAGES; pageNumber++) {
    const page: ConversationPage = await invoke<ConversationPage>('engine_load_timeline', {
      session,
      page: { cursor, limit: PAGE_LIMIT },
    })
    records.push(...page.records)
    if (!page.nextCursor || page.nextCursor === cursor) {
      return { records, nextCursor: null }
    }
    cursor = page.nextCursor
  }
  throw new Error('Timeline pagination exceeded the safety limit')
}

export function sessionActions(session: SessionRef): Promise<SessionActions> {
  return invoke('engine_session_actions', { session })
}

export function runtimeSnapshots(): Promise<RuntimeSnapshot[]> {
  return invoke('engine_runtime_snapshots')
}

export function listModels(instance: EngineInstanceId): Promise<ModelDescriptor[]> {
  return invoke('engine_list_models', { instance })
}

export async function listAssets(instance: EngineInstanceId, kind: string): Promise<FacetItem[]> {
  const result: FacetItem[] = []
  let cursor: string | null = null
  for (let pageNumber = 0; pageNumber < MAX_PAGES; pageNumber++) {
    const page: { items: FacetItem[]; nextCursor: string | null } = await invoke('engine_list_assets', {
      instance,
      query: { kind, cursor, limit: PAGE_LIMIT },
    })
    result.push(...page.items)
    if (!page.nextCursor || page.nextCursor === cursor) return result
    cursor = page.nextCursor
  }
  throw new Error('Asset pagination exceeded the safety limit')
}

export function attachSession(session: SessionRef, options: Record<string, unknown> = {}) {
  return invoke<{ session: SessionRef; runtimeId: unknown; generation: number }>('engine_attach_session', {
    session,
    options: { options },
  })
}

export function createSession(project: ProjectRef, cwd: string | null, options: Record<string, unknown> = {}) {
  return invoke<{ session: SessionRef; runtimeId: unknown; generation: number }>('engine_create_session', {
    request: { project, cwd, options },
  })
}

export function forkSession(session: SessionRef, lastTurnId: string | null = null, options: Record<string, unknown> = {}) {
  return invoke<{ session: SessionRef; runtimeId: unknown; generation: number }>('engine_fork_session', {
    request: { session, lastTurnId, options },
  })
}

export function startTurn(session: SessionRef, text: string, options: Record<string, unknown> = {}) {
  return startTurnWithInput(session, [{ kind: 'text', text }], options)
}

export function startTurnWithInput(session: SessionRef, input: RuntimeInputItem[], options: Record<string, unknown> = {}) {
  return invoke<TurnHandle>('engine_start_turn', {
    session,
    request: { input, options },
  })
}

export function sendInputWhileRunning(
  session: SessionRef,
  runtimeId: unknown,
  nativeTurnId: string,
  input: RuntimeInputItem[],
) {
  return invoke('engine_send_input_while_running', {
    turn: { session, runtimeId, nativeTurnId },
    input,
  })
}

export function interruptTurn(session: SessionRef, runtimeId: unknown, nativeTurnId: string) {
  return invoke('engine_interrupt_turn', {
    turn: { session, runtimeId, nativeTurnId },
  })
}

export function respondInteraction(request: InteractionRef, decision: string, payload?: unknown) {
  return invoke('engine_respond_interaction', {
    request,
    response: { decision, payload: payload ?? null },
  })
}

export function resolveAsset(session: SessionRef, nativeId: string, preview = false) {
  return invoke<{ mediaType: string; bytes: number[] }>('engine_resolve_asset', {
    asset: { session, nativeId },
    preview,
  })
}

export function segmentText(segment: EngineSegment): string {
  if (segment.kind === 'text' || segment.kind === 'reasoning') return segment.text
  if (segment.kind === 'commandExecution') return [segment.command, segment.output].filter(Boolean).join('\n')
  if (segment.kind === 'fileChange') return segment.changes.map(change => change.diff || `${change.kind}: ${change.path}`).join('\n')
  if (segment.kind === 'toolCall') return `${segment.name}\n${JSON.stringify(segment.input, null, 2)}`
  if (segment.kind === 'toolResult') return typeof segment.content === 'string' ? segment.content : JSON.stringify(segment.content, null, 2)
  if (segment.kind === 'unknown') return segment.summary || segment.typeName
  return segment.title || segment.mediaType
}

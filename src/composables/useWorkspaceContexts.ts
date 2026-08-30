import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Project, SessionSummary, WorkspaceContext } from '@/types'
import type { SessionRef } from '@/engines/types'
import { sessionUiId } from '@/engines/integration'

interface WorkspaceResolveResult {
  session: SessionRef
  context: WorkspaceContext | null
}

const contexts = ref<Record<string, WorkspaceContext>>({})
let resolveGeneration = 0

function applyKnownContexts(projects: Project[]) {
  for (const session of projects.flatMap(project => project.sessions)) {
    const context = contexts.value[session.id]
    if (context) session.workspace_context = context
  }
}

async function refreshWorkspaceContexts(projects: Project[]): Promise<void> {
  applyKnownContexts(projects)
  const requests = projects.flatMap(project => project.sessions
    .filter((session): session is SessionSummary & { reference: SessionRef } => !!session.reference)
    .map(session => ({
      session: session.reference,
      cwd: session.cwd,
      projectPath: project.source_path ?? null,
    })))
  const generation = ++resolveGeneration
  if (!requests.length) {
    contexts.value = {}
    return
  }

  const results = await invoke<WorkspaceResolveResult[]>('resolve_workspace_contexts', { requests })
  if (generation !== resolveGeneration) return

  const next: Record<string, WorkspaceContext> = {}
  for (const result of results) {
    if (result.context) next[sessionUiId(result.session)] = result.context
  }
  contexts.value = next
  for (const session of projects.flatMap(project => project.sessions)) {
    const context = next[session.id]
    if (context) session.workspace_context = context
    else delete session.workspace_context
  }
}

function workspaceForSession(session: SessionSummary): WorkspaceContext | undefined {
  return contexts.value[session.id] ?? session.workspace_context
}

function workspaceUnavailable(session: SessionSummary): boolean {
  const context = workspaceForSession(session)
  return context?.kind === 'legacy' && !context.available
}

function workspaceCwd(session: SessionSummary): string | null {
  const context = workspaceForSession(session)
  if (context?.kind === 'legacy' && !context.available) return null
  if (context?.kind === 'linked' && context.available) return context.worktreeRoot
  return session.cwd
}

function workspaceFileRoot(session: SessionSummary): string | null {
  const context = workspaceForSession(session)
  if (context?.kind === 'linked' || context?.kind === 'legacy') return context.worktreeRoot
  return session.cwd
}

export function useWorkspaceContexts() {
  return {
    contexts,
    refreshWorkspaceContexts,
    workspaceForSession,
    workspaceUnavailable,
    workspaceCwd,
    workspaceFileRoot,
  }
}

import type { Project, SessionSummary } from '@/types'
import type { SessionRef } from './types'

const sessions = new Map<string, SessionSummary>()

export function indexEngineSessions(projects: Project[]) {
  sessions.clear()
  for (const project of projects) {
    for (const session of project.sessions) sessions.set(session.id, session)
  }
}

export function resolveSession(sessionId: string): SessionSummary | undefined {
  return sessions.get(sessionId)
}

export function resolveSessionRef(sessionId: string): SessionRef | undefined {
  return sessions.get(sessionId)?.reference
}

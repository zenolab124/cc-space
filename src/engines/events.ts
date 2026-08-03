import type { EngineInstanceId, ProjectRef, SessionRef } from './types'

export interface SourceChangeEnvelope {
  instance: EngineInstanceId
  change: {
    kind: 'projectsChanged' | 'sessionChanged' | 'sessionRemoved' | 'fullRefresh'
    project: ProjectRef | null
    session: SessionRef | null
  }
}

import type { ConversationRecord, EngineSegment } from './types'

export type EngineAsyncTaskState = 'running' | 'completed' | 'failed' | 'unknown'

export interface EngineAsyncTask {
  key: string
  threadId: string
  title: string
  prompt: string | null
  model: string | null
  effort: string | null
  state: EngineAsyncTaskState
  message: string | null
  updatedAt: string | null
}

const COLLAB_TOOLS = new Set(['spawnAgent', 'sendInput', 'resumeAgent', 'wait', 'closeAgent'])

function objectValue(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {}
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null
}

function receiverIds(input: Record<string, unknown>): string[] {
  const explicit = Array.isArray(input.receiverThreadIds)
    ? input.receiverThreadIds.filter((value): value is string => typeof value === 'string' && !!value)
    : []
  if (explicit.length) return explicit
  return Object.keys(objectValue(input.agentsStates))
}

function stateFromAgent(value: unknown): { state: EngineAsyncTaskState; message: string | null } | null {
  const state = objectValue(value)
  const status = stringValue(state.status)
  const message = stringValue(state.message)
  if (!status) return null
  if (status === 'pendingInit' || status === 'running') return { state: 'running', message }
  if (status === 'completed' || status === 'shutdown') return { state: 'completed', message }
  if (status === 'errored' || status === 'interrupted') return { state: 'failed', message }
  return { state: 'unknown', message }
}

function titleFromPrompt(prompt: string | null, threadId: string): string {
  if (!prompt) return threadId.slice(0, 8)
  const firstLine = prompt.split('\n').find(line => line.trim())?.trim() ?? prompt
  return firstLine.length > 96 ? `${firstLine.slice(0, 93)}…` : firstLine
}

function isCollabSegment(segment: EngineSegment): segment is Extract<EngineSegment, { kind: 'toolCall' }> {
  return segment.kind === 'toolCall' && COLLAB_TOOLS.has(segment.name)
}

export function buildEngineAsyncTasks(records: ConversationRecord[]): EngineAsyncTask[] {
  const tasks = new Map<string, EngineAsyncTask>()
  const order: string[] = []

  for (const record of records) {
    for (const segment of record.segments) {
      if (!isCollabSegment(segment)) continue
      const input = objectValue(segment.input)
      const ids = receiverIds(input)
      const prompt = stringValue(input.prompt)
      const model = stringValue(input.model)
      const effort = stringValue(input.reasoningEffort)
      const callStatus = stringValue(input.status)
      const agentsStates = objectValue(input.agentsStates)

      for (const threadId of ids) {
        let task = tasks.get(threadId)
        if (!task) {
          task = {
            key: threadId,
            threadId,
            title: titleFromPrompt(prompt, threadId),
            prompt,
            model,
            effort,
            state: callStatus === 'failed' ? 'failed' : 'running',
            message: null,
            updatedAt: record.timestamp,
          }
          tasks.set(threadId, task)
          order.push(threadId)
        } else {
          if (prompt) {
            task.prompt = prompt
            task.title = titleFromPrompt(prompt, threadId)
          }
          if (model) task.model = model
          if (effort) task.effort = effort
          task.updatedAt = record.timestamp ?? task.updatedAt
        }

        const agentState = stateFromAgent(agentsStates[threadId])
        if (agentState) {
          task.state = agentState.state
          task.message = agentState.message
        } else if (callStatus === 'failed') {
          task.state = 'failed'
        } else if (segment.name === 'closeAgent' && callStatus === 'completed') {
          task.state = 'completed'
        }
      }
    }
  }

  return order.map(key => tasks.get(key)!)
}

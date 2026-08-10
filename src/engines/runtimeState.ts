import { computed, ref, type ComputedRef, type Ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { listEngines, runtimeSnapshots, segmentText } from './client'
import { sessionUiId } from './integration'
import { resolveEnginePresentation } from './presentation'
import { isEngineThoughtSegment } from './processGroups'
import type {
  EngineSegment,
  RuntimeEventEnvelope,
  RuntimeSnapshot,
} from './types'

export interface EngineRuntimeTailLine {
  kind: 'text' | 'tool' | 'error'
  text: string
}

interface EngineRuntimeView {
  snapshot: RuntimeSnapshot | null
  tail: EngineRuntimeTailLine[]
  startedAt: number | null
}

const snapshots = ref<Map<string, RuntimeSnapshot>>(new Map())
const tails = ref<Map<string, EngineRuntimeTailLine[]>>(new Map())
const startedAt = ref<Map<string, number>>(new Map())
let initialized = false

function updateMap<T>(state: Ref<Map<string, T>>, key: string, value: T | null) {
  const next = new Map(state.value)
  if (value === null) next.delete(key)
  else next.set(key, value)
  state.value = next
}

function storeSnapshot(snapshot: RuntimeSnapshot) {
  const sessionId = sessionUiId(snapshot.session)
  if (snapshot.phase === 'detached') {
    updateMap(snapshots, sessionId, null)
    updateMap(tails, sessionId, null)
    updateMap(startedAt, sessionId, null)
    return
  }
  updateMap(snapshots, sessionId, snapshot)
  if (snapshot.phase === 'running' || snapshot.phase === 'awaitingInteraction') {
    if (!startedAt.value.has(sessionId)) updateMap(startedAt, sessionId, Date.now())
  } else if (snapshot.phase === 'idle' || snapshot.phase === 'exited') {
    updateMap(startedAt, sessionId, null)
  }
}

export function engineRuntimeSnapshot(sessionId: string): RuntimeSnapshot | null {
  return snapshots.value.get(sessionId) ?? null
}

function tailKind(segment: EngineSegment): EngineRuntimeTailLine['kind'] {
  return segment.kind === 'text' || segment.kind === 'reasoning' ? 'text' : 'tool'
}

function appendTail(sessionId: string, segment: EngineSegment) {
  const text = segmentText(segment).trim()
  if (!text) return
  const kind = tailKind(segment)
  const current = [...(tails.value.get(sessionId) ?? [])]
  if (current.length && current[current.length - 1].kind === kind) {
    current[current.length - 1] = {
      kind,
      text: `${current[current.length - 1].text}${text}`,
    }
  } else {
    current.push({ kind, text })
  }
  const normalized = current
    .flatMap(line => line.text.split(/\r?\n/).map(text => ({ kind: line.kind, text })))
    .filter(line => line.text.trim())
    .map(line => ({ ...line, text: line.text.slice(-240) }))
    .slice(-3)
  updateMap(tails, sessionId, normalized)
}

export function shouldIncludeRuntimeTailSegment(engineId: string, segment: EngineSegment): boolean {
  return resolveEnginePresentation(engineId, null).showThoughtProcess
    || !isEngineThoughtSegment(segment)
}

function applyEvent(envelope: RuntimeEventEnvelope) {
  const sessionId = sessionUiId(envelope.session)
  const event = envelope.event
  if (event.kind === 'turnStarted') {
    updateMap(startedAt, sessionId, Date.now())
    updateMap(tails, sessionId, [])
  } else if (event.kind === 'itemDelta' && event.segment) {
    if (shouldIncludeRuntimeTailSegment(envelope.session.engine.engineId, event.segment)) {
      appendTail(sessionId, event.segment)
    }
  } else if (event.kind === 'runtimeError') {
    updateMap(tails, sessionId, [{ kind: 'error', text: String(event.message ?? '') }])
  }
}

export async function initEngineRuntimeState(): Promise<void> {
  if (initialized) return
  initialized = true
  try {
    await listEngines()
  } catch (_) {
    // 注册表暂不可用时仍安装监听；结构化身份是标准界面的安全默认值。
  }
  await listen<RuntimeSnapshot>('engine-runtime-snapshot', event => storeSnapshot(event.payload))
  await listen<RuntimeEventEnvelope[]>('engine-runtime-events', event => {
    for (const envelope of event.payload) applyEvent(envelope)
  })
  try {
    for (const snapshot of await runtimeSnapshots()) storeSnapshot(snapshot)
  } catch (_) {
    // 运行时尚未初始化时保持空仓库；后续事件会自动收敛。
  }
}

export function useEngineRuntimeState(
  sessionId: Ref<string | null> | ComputedRef<string | null>,
): ComputedRef<EngineRuntimeView> {
  return computed(() => {
    const id = sessionId.value
    return {
      snapshot: id ? snapshots.value.get(id) ?? null : null,
      tail: id ? tails.value.get(id) ?? [] : [],
      startedAt: id ? startedAt.value.get(id) ?? null : null,
    }
  })
}

import { smoothTakeForElapsed, streamRenderInterval } from '@/lib/streamRenderPolicy'
import type { EngineSegment, RuntimeEventEnvelope } from './types'

const SMOOTH_WINDOW_MS = 300
const SMOOTH_WARMUP_MS = 200
const FLOOD_CHARS = 20_000

type ItemDeltaEnvelope = RuntimeEventEnvelope & {
  event: Extract<RuntimeEventEnvelope['event'], { kind: 'itemDelta' }>
}

interface PendingDelta {
  turnId: string
  itemId: string
  envelope: ItemDeltaEnvelope
  text: string
  bornAt: number
  lastFlushAt: number | null
  emitted: boolean
}

export interface RuntimeDeltaShaperSnapshot {
  pending: boolean
  turnIds: Set<string>
}

export interface RuntimeDeltaShaper {
  push(envelope: RuntimeEventEnvelope): void
  reset(): void
  snapshot(): RuntimeDeltaShaperSnapshot
}

interface RuntimeDeltaShaperOptions {
  deliver(envelope: RuntimeEventEnvelope): void
  onStateChange?(snapshot: RuntimeDeltaShaperSnapshot): void
  now?: () => number
  documentVisible?: () => boolean
  schedule?: (callback: () => void, delay: number) => number
  cancel?: (timer: number) => void
}

function segmentBufferText(segment: EngineSegment): string | null {
  if (segment.kind === 'text' || segment.kind === 'reasoning') return segment.text
  if (segment.kind === 'commandExecution') return segment.output
  return null
}

function segmentKey(segment: EngineSegment): string | null {
  if (segment.kind === 'text') return `text:${segment.phase ?? 'legacy'}`
  if (segment.kind === 'reasoning') return `reasoning:${segment.visibility}`
  if (segment.kind === 'commandExecution') return `command:${segment.id}`
  return null
}

function segmentWithChunk(segment: EngineSegment, chunk: string, emitted: boolean): EngineSegment {
  if (segment.kind === 'text') return { ...segment, text: chunk }
  if (segment.kind === 'reasoning') return { ...segment, text: chunk }
  if (segment.kind === 'commandExecution') {
    return {
      ...segment,
      command: emitted ? '' : segment.command,
      cwd: emitted ? null : segment.cwd,
      output: chunk,
    }
  }
  return segment
}

function mergeSegmentMetadata(current: EngineSegment, incoming: EngineSegment): EngineSegment {
  if (current.kind === 'commandExecution' && incoming.kind === 'commandExecution') {
    return {
      ...incoming,
      command: incoming.command || current.command,
      cwd: incoming.cwd ?? current.cwd,
    }
  }
  return incoming
}

/**
 * 对引擎中立 itemDelta 做到达率自适应整形。协议顺序保持不变；item/turn 完成
 * 事件会等对应缓冲排空后再交给状态机，避免结束时瞬间倾倒残余文本。
 */
export function createRuntimeDeltaShaper(options: RuntimeDeltaShaperOptions): RuntimeDeltaShaper {
  const now = options.now ?? (() => performance.now())
  const documentVisible = options.documentVisible ?? (() => document.visibilityState === 'visible')
  const schedule = options.schedule ?? ((callback, delay) => window.setTimeout(callback, delay))
  const cancel = options.cancel ?? (timer => window.clearTimeout(timer))
  const pending = new Map<string, PendingDelta>()
  const itemCompletions = new Map<string, RuntimeEventEnvelope>()
  const turnCompletions = new Map<string, RuntimeEventEnvelope>()
  let timer: number | null = null

  function itemKey(turnId: string, itemId: string): string {
    return `${turnId}\u001f${itemId}`
  }

  function stateSnapshot(): RuntimeDeltaShaperSnapshot {
    return {
      pending: pending.size > 0,
      turnIds: new Set([...pending.values()].map(entry => entry.turnId)),
    }
  }

  function publishState() {
    options.onStateChange?.(stateSnapshot())
  }

  function hasPendingItem(turnId: string, itemId: string): boolean {
    for (const entry of pending.values()) {
      if (entry.turnId === turnId && entry.itemId === itemId) return true
    }
    return false
  }

  function hasPendingTurn(turnId: string): boolean {
    for (const entry of pending.values()) {
      if (entry.turnId === turnId) return true
    }
    return false
  }

  function deliverDeferred() {
    for (const [key, envelope] of [...itemCompletions]) {
      const event = envelope.event
      if (event.kind !== 'itemCompleted' || hasPendingItem(event.turnId, event.itemId)) continue
      itemCompletions.delete(key)
      options.deliver(envelope)
    }
    for (const [turnId, envelope] of [...turnCompletions]) {
      if (hasPendingTurn(turnId)) continue
      turnCompletions.delete(turnId)
      options.deliver(envelope)
    }
  }

  function cadence(): number {
    return streamRenderInterval('active', documentVisible())
  }

  function scheduleTick() {
    if (timer !== null || pending.size === 0) return
    timer = schedule(tick, cadence())
  }

  function tick() {
    timer = null
    const at = now()
    const visible = documentVisible()
    for (const [key, entry] of pending) {
      if (visible && at - entry.bornAt < SMOOTH_WARMUP_MS) continue
      const elapsed = entry.lastFlushAt === null ? cadence() : Math.max(1, at - entry.lastFlushAt)
      const take = !visible || entry.text.length > FLOOD_CHARS
        ? entry.text.length
        : smoothTakeForElapsed(entry.text.length, elapsed, SMOOTH_WINDOW_MS)
      const chunk = entry.text.slice(0, take)
      entry.text = entry.text.slice(take)
      entry.lastFlushAt = at
      const event = entry.envelope.event
      options.deliver({
        ...entry.envelope,
        event: {
          ...event,
          segment: segmentWithChunk(event.segment, chunk, entry.emitted),
        },
      })
      entry.emitted = true
      if (!entry.text) pending.delete(key)
    }
    deliverDeferred()
    publishState()
    scheduleTick()
  }

  function push(envelope: RuntimeEventEnvelope) {
    const event = envelope.event
    if (event.kind === 'itemDelta') {
      const text = segmentBufferText(event.segment)
      const kind = segmentKey(event.segment)
      if (text && kind) {
        const key = `${itemKey(event.turnId, event.itemId)}\u001f${kind}`
        const current = pending.get(key)
        if (current) {
          current.text += text
          const nextEnvelope = envelope as ItemDeltaEnvelope
          current.envelope = {
            ...nextEnvelope,
            event: {
              ...nextEnvelope.event,
              segment: mergeSegmentMetadata(current.envelope.event.segment, nextEnvelope.event.segment),
            },
          }
        } else {
          pending.set(key, {
            turnId: event.turnId,
            itemId: event.itemId,
            envelope: envelope as ItemDeltaEnvelope,
            text,
            bornAt: now(),
            lastFlushAt: null,
            emitted: false,
          })
        }
        publishState()
        scheduleTick()
        return
      }
    }
    if (event.kind === 'itemCompleted' && hasPendingItem(event.turnId, event.itemId)) {
      itemCompletions.set(itemKey(event.turnId, event.itemId), envelope)
      return
    }
    if (event.kind === 'turnCompleted' && hasPendingTurn(event.turnId)) {
      turnCompletions.set(event.turnId, envelope)
      return
    }
    options.deliver(envelope)
  }

  function reset() {
    if (timer !== null) cancel(timer)
    timer = null
    pending.clear()
    itemCompletions.clear()
    turnCompletions.clear()
    publishState()
  }

  return { push, reset, snapshot: stateSnapshot }
}

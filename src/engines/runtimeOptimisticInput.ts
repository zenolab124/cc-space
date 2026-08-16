import { computed, ref, type ComputedRef } from 'vue'
import { sessionUiId } from './integration'
import {
  bindOptimisticUserTurn,
  createOptimisticUserRecord,
  reconcileLiveRecords,
  type OptimisticUserImage,
} from './runtimeTimeline'
import type { ConversationRecord, SessionRef } from './types'

const recordsBySession = ref<Map<string, ConversationRecord[]>>(new Map())
let optimisticSequence = 0

function replaceSessionRecords(sessionId: string, records: ConversationRecord[]) {
  const next = new Map(recordsBySession.value)
  if (records.length) next.set(sessionId, records)
  else next.delete(sessionId)
  recordsBySession.value = next
}

/** 供共享输入等会话外控制器写入，详情组件会自动把记录并入实时树。 */
export function stageRuntimeOptimisticInput(
  session: SessionRef,
  text: string,
  images: OptimisticUserImage[] = [],
  turnId: string | null = null,
): string {
  optimisticSequence += 1
  const id = `pending-user-${Date.now()}-${optimisticSequence}`
  const sessionId = sessionUiId(session)
  const current = recordsBySession.value.get(sessionId) ?? []
  replaceSessionRecords(sessionId, [
    ...current,
    createOptimisticUserRecord(session, id, text, images, turnId),
  ])
  return id
}

export function bindRuntimeOptimisticInput(
  session: SessionRef,
  recordId: string,
  turnId: string,
) {
  const sessionId = sessionUiId(session)
  const current = recordsBySession.value.get(sessionId) ?? []
  replaceSessionRecords(sessionId, bindOptimisticUserTurn(current, recordId, turnId))
}

/** 事件可能早于 invoke 返回；用有序 turnStarted 先绑定最近一条未绑定输入。 */
export function bindLatestRuntimeOptimisticInput(session: SessionRef, turnId: string) {
  const sessionId = sessionUiId(session)
  const current = recordsBySession.value.get(sessionId) ?? []
  const pending = [...current].reverse().find(record => record.role === 'user' && !record.turnId)
  if (pending) replaceSessionRecords(sessionId, bindOptimisticUserTurn(current, pending.id, turnId))
}

export function removeRuntimeOptimisticInput(session: SessionRef, recordId: string) {
  const sessionId = sessionUiId(session)
  const current = recordsBySession.value.get(sessionId) ?? []
  replaceSessionRecords(sessionId, current.filter(record => record.id !== recordId))
}

/** 历史源接管后及时释放已落账记录，避免模块级状态随会话轮次增长。 */
export function reconcileRuntimeOptimisticInputs(
  session: SessionRef,
  persisted: ConversationRecord[],
) {
  const sessionId = sessionUiId(session)
  const current = recordsBySession.value.get(sessionId) ?? []
  if (!current.length) return
  replaceSessionRecords(sessionId, reconcileLiveRecords(persisted, current))
}

export function clearRuntimeOptimisticInputs(sessionId: string) {
  replaceSessionRecords(sessionId, [])
}

export function useRuntimeOptimisticInputs(
  session: ComputedRef<SessionRef | null | undefined>,
): ComputedRef<ConversationRecord[]> {
  return computed(() => {
    const value = session.value
    return value ? recordsBySession.value.get(sessionUiId(value)) ?? [] : []
  })
}

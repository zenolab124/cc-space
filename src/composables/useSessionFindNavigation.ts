import { computed, ref, watch, type ComputedRef } from 'vue'
import {
  findSessionTextMatches,
  type SessionFindRequest,
  type SessionFindStatus,
} from '@/utils/sessionFind'

/**
 * 会话内查找的引擎中立导航状态。宿主只提供完整消息组文本与虚拟列表定位函数。
 */
export function useSessionFindNavigation(
  request: ComputedRef<SessionFindRequest | null>,
  groupTexts: ComputedRef<readonly string[]>,
  revealGroup: (groupIndex: number) => void,
  reportStatus: (status: SessionFindStatus) => void,
) {
  const matches = computed(() => findSessionTextMatches(
    groupTexts.value,
    request.value?.active ? request.value.query : '',
  ))
  const currentMatchIndex = ref(-1)
  let appliedQueryKey = ''

  const matchingGroupIndexes = computed(() => new Set(matches.value.map(match => match.groupIndex)))
  const activeGroupIndex = computed(() => (
    currentMatchIndex.value >= 0
      ? matches.value[currentMatchIndex.value]?.groupIndex ?? -1
      : -1
  ))

  function status(): SessionFindStatus {
    return {
      current: currentMatchIndex.value >= 0 ? currentMatchIndex.value + 1 : 0,
      total: matches.value.length,
    }
  }

  function report() {
    reportStatus(status())
  }

  function revealCurrent() {
    const match = matches.value[currentMatchIndex.value]
    if (match) revealGroup(match.groupIndex)
  }

  watch(
    [() => request.value?.active ?? false, () => request.value?.query ?? ''],
    ([active, query]) => {
      appliedQueryKey = active ? query : ''
      currentMatchIndex.value = active && matches.value.length ? 0 : -1
      report()
      if (currentMatchIndex.value >= 0) revealCurrent()
    },
    { immediate: true, flush: 'sync' },
  )

  // 流式落账或历史刷新改变索引时保留当前序号；只在越界时收回末项。
  watch(matches, (nextMatches, previousMatches) => {
    const queryKey = request.value?.active ? request.value.query : ''
    if (queryKey !== appliedQueryKey) return
    const previousGroupIndex = previousMatches[currentMatchIndex.value]?.groupIndex ?? -1
    if (!matches.value.length) currentMatchIndex.value = -1
    else if (currentMatchIndex.value < 0) currentMatchIndex.value = 0
    else currentMatchIndex.value = Math.min(currentMatchIndex.value, matches.value.length - 1)
    report()
    if (
      currentMatchIndex.value >= 0
      && nextMatches[currentMatchIndex.value]?.groupIndex !== previousGroupIndex
    ) revealCurrent()
  })

  watch(() => request.value?.navigationRevision ?? 0, (revision, previousRevision) => {
    if (revision === previousRevision || !request.value?.active || !matches.value.length) return
    const direction = request.value.direction
    if (direction === 'previous') {
      currentMatchIndex.value = (currentMatchIndex.value - 1 + matches.value.length) % matches.value.length
    } else if (direction === 'next') {
      currentMatchIndex.value = (currentMatchIndex.value + 1) % matches.value.length
    } else {
      currentMatchIndex.value = 0
    }
    report()
    revealCurrent()
  })

  return {
    matchingGroupIndexes,
    activeGroupIndex,
  }
}

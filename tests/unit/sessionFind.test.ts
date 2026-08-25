import { describe, expect, it } from 'vitest'
import { computed, nextTick, ref } from 'vue'
import { findSessionTextMatches } from '../../src/utils/sessionFind'
import { useSessionFindNavigation } from '../../src/composables/useSessionFindNavigation'
import type { SessionFindRequest, SessionFindStatus } from '../../src/utils/sessionFind'

describe('session find text index', () => {
  it('counts every case-insensitive occurrence across complete message groups', () => {
    expect(findSessionTextMatches([
      'Alpha beta alpha',
      'No match here',
      'ALPHA',
    ], 'alpha')).toEqual([
      { groupIndex: 0, offset: 0 },
      { groupIndex: 0, offset: 11 },
      { groupIndex: 2, offset: 0 },
    ])
  })

  it('returns no matches for an empty or whitespace-only query', () => {
    expect(findSessionTextMatches(['content'], '')).toEqual([])
    expect(findSessionTextMatches(['content'], '   ')).toEqual([])
  })

  it('keeps matching independent of rendered DOM availability', () => {
    const groups = Array.from({ length: 500 }, (_, index) => `message ${index}`)

    expect(findSessionTextMatches(groups, 'message 420'))
      .toEqual([{ groupIndex: 420, offset: 0 }])
  })

  it('wraps next and previous navigation while reporting one-based status', async () => {
    const request = ref<SessionFindRequest>({
      active: false,
      query: '',
      navigationRevision: 0,
      direction: 'first',
    })
    const groups = ref(['Alpha alpha', 'other', 'alpha'])
    const statuses: SessionFindStatus[] = []
    const revealed: number[] = []
    useSessionFindNavigation(
      computed(() => request.value),
      computed(() => groups.value),
      index => revealed.push(index),
      status => statuses.push(status),
    )

    request.value = { ...request.value, active: true, query: 'alpha' }
    await nextTick()
    expect(statuses.at(-1)).toEqual({ current: 1, total: 3 })
    expect(revealed.at(-1)).toBe(0)

    request.value = { ...request.value, direction: 'next', navigationRevision: 1 }
    await nextTick()
    expect(statuses.at(-1)).toEqual({ current: 2, total: 3 })
    expect(revealed.at(-1)).toBe(0)

    request.value = { ...request.value, direction: 'previous', navigationRevision: 2 }
    await nextTick()
    expect(statuses.at(-1)).toEqual({ current: 1, total: 3 })
  })
})

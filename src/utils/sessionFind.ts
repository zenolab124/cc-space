export type SessionFindDirection = 'first' | 'next' | 'previous'

export interface SessionFindRequest {
  active: boolean
  query: string
  navigationRevision: number
  direction: SessionFindDirection
}

export interface SessionFindStatus {
  current: number
  total: number
}

export interface SessionFindMatch {
  groupIndex: number
  offset: number
}

/**
 * 在完整的中立消息组索引中查找，不依赖当前已挂载的 DOM 节点。
 * 同一消息组内的多次命中会分别计数，导航时仍落到该消息组。
 */
export function findSessionTextMatches(
  groupTexts: readonly string[],
  query: string,
): SessionFindMatch[] {
  if (!query.trim()) return []

  const needle = query.toLocaleLowerCase()
  const matches: SessionFindMatch[] = []
  for (const [groupIndex, text] of groupTexts.entries()) {
    const haystack = text.toLocaleLowerCase()
    let offset = 0
    while (offset <= haystack.length - needle.length) {
      const found = haystack.indexOf(needle, offset)
      if (found < 0) break
      matches.push({ groupIndex, offset: found })
      offset = found + needle.length
    }
  }
  return matches
}

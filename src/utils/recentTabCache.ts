/**
 * 维护最近访问 Tab 的有界缓存。返回顺序按最近访问优先排列。
 */
export function touchRecentTab(
  cachedIds: readonly string[],
  activeId: string,
  validIds: ReadonlySet<string>,
  limit: number,
): string[] {
  if (limit < 1 || !validIds.has(activeId)) return []

  return [
    activeId,
    ...cachedIds.filter(id => id !== activeId && validIds.has(id)),
  ].slice(0, limit)
}

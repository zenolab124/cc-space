export interface WorkbenchTabLike {
  id: string
}

/** 按稳定 ID 重排 Tab；活动态由调用方持有的 ID 自然保持。 */
export function reorderWorkbenchTabs<T extends WorkbenchTabLike>(
  tabs: readonly T[],
  sourceId: string,
  targetId: string,
): T[] {
  const fromIndex = tabs.findIndex(tab => tab.id === sourceId)
  const toIndex = tabs.findIndex(tab => tab.id === targetId)
  if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return [...tabs]

  const next = [...tabs]
  const [moved] = next.splice(fromIndex, 1)
  next.splice(toIndex, 0, moved)
  return next
}

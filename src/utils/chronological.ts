/**
 * 时间线集合统一约定：源数组按从旧到新排列，末项就是最新项。
 */
export function latestChronologicalItem<T>(items: readonly T[]): T | null {
  return items.length > 0 ? items[items.length - 1] : null
}

/** 返回从新到旧的副本，不改动源时间线。 */
export function newestFirst<T>(items: readonly T[]): T[] {
  return [...items].reverse()
}

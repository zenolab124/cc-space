/** 新列只取得自己的默认宽度，不改动已存在列。 */
export function insertColumnWidth(
  widths: readonly number[],
  index: number,
  defaultWidth: number,
): number[] {
  const next = [...widths]
  const target = Math.max(0, Math.min(Math.trunc(index), next.length))
  next.splice(target, 0, Math.max(0, Math.round(defaultWidth)))
  return next
}

/** 移除列只删除对应宽度，不把空余空间分配给相邻列。 */
export function removeColumnWidth(widths: readonly number[], index: number): number[] {
  if (index < 0 || index >= widths.length) return [...widths]
  const next = [...widths]
  next.splice(index, 1)
  return next
}

/** 拖动分隔线只调整目标列，后续列整体平移。 */
export function resizeColumnWidth(
  widths: readonly number[],
  index: number,
  desiredWidth: number,
  minWidth: number,
): number[] {
  if (index < 0 || index >= widths.length) return [...widths]
  const next = [...widths]
  next[index] = Math.max(Math.round(minWidth), Math.round(desiredWidth))
  return next
}

/**
 * 在列宽总和小于可用宽度时按原比例铺满；已经溢出时保持原宽度。
 * 使用整数像素并补齐舍入余数，避免末列残留 1px 空隙。
 */
export function fillColumnWidthsProportionally(widths: readonly number[], availableWidth: number): number[] {
  if (widths.length === 0) return []

  const total = widths.reduce((sum, width) => sum + width, 0)
  const target = Math.max(0, Math.round(availableWidth))
  if (total <= 0 || total >= target) return [...widths]

  const scaled = widths.map((width, index) => {
    const exact = width * target / total
    return { index, width: Math.floor(exact), fraction: exact - Math.floor(exact) }
  })
  let remainder = target - scaled.reduce((sum, item) => sum + item.width, 0)

  scaled
    .slice()
    .sort((a, b) => b.fraction - a.fraction || a.index - b.index)
    .forEach((item) => {
      if (remainder <= 0) return
      scaled[item.index].width += 1
      remainder -= 1
    })

  return scaled.map(item => item.width)
}

import { ref } from 'vue'
import { useWorkbench } from './useWorkbench'

/**
 * 列右缘分隔线拖拽(普通多列与赛马共用):
 * 像素级调整左列宽度;Shift 按住时调整全局最小列宽(全列重置为该宽)
 */
export function useColumnResize() {
  const { activeTab, updateColumnSize, setMinColumnWidth, minColumnWidth } = useWorkbench()
  const dragging = ref(false)
  const shiftDragging = ref(false)

  function onDividerMouseDown(e: MouseEvent, index: number) {
    e.preventDefault()
    const tab = activeTab.value
    const columnElement = (e.currentTarget as HTMLElement | null)?.closest<HTMLElement>('[data-workbench-column]')
    const renderedColumns = columnElement?.parentElement
      ? Array.from(columnElement.parentElement.children)
          .filter((element): element is HTMLElement => (
            element instanceof HTMLElement && element.hasAttribute('data-workbench-column')
          ))
      : []
    // 弹性铺满只影响渲染宽度；开始手调前先实体化，避免首帧跳回持久化基准宽。
    if (renderedColumns.length === tab.columnSizes.length) {
      const renderedWidths = renderedColumns.map(column => Math.round(column.getBoundingClientRect().width))
      if (renderedWidths.every(width => width > 0)) tab.columnSizes = renderedWidths
    }

    dragging.value = true
    const isShift = e.shiftKey
    shiftDragging.value = isShift
    const startX = e.clientX

    if (isShift) {
      const startMin = minColumnWidth.value
      const onMouseMove = (ev: MouseEvent) => {
        const delta = ev.clientX - startX
        const newMin = startMin + delta
        setMinColumnWidth(newMin)
        tab.columnSizes = tab.columnSizes.map(() => minColumnWidth.value)
      }
      const onMouseUp = () => {
        dragging.value = false
        shiftDragging.value = false
        document.removeEventListener('mousemove', onMouseMove)
        document.removeEventListener('mouseup', onMouseUp)
      }
      document.addEventListener('mousemove', onMouseMove)
      document.addEventListener('mouseup', onMouseUp)
    } else {
      const startWidth = tab.columnSizes[index]
      const onMouseMove = (ev: MouseEvent) => {
        const delta = ev.clientX - startX
        updateColumnSize(tab.id, index, startWidth + delta)
      }
      const onMouseUp = () => {
        dragging.value = false
        document.removeEventListener('mousemove', onMouseMove)
        document.removeEventListener('mouseup', onMouseUp)
      }
      document.addEventListener('mousemove', onMouseMove)
      document.addEventListener('mouseup', onMouseUp)
    }
  }

  return { dragging, shiftDragging, onDividerMouseDown }
}

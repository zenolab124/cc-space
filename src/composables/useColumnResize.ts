import { ref, type Ref } from 'vue'
import { useWorkbench, type WorkbenchTab } from './useWorkbench'

/**
 * 列右缘分隔线拖拽(普通多列与赛马共用):
 * 像素级调整目标列宽度，其他列保持不变。
 */
export function useColumnResize(tabRef: Readonly<Ref<WorkbenchTab>>) {
  const { updateColumnSize } = useWorkbench()
  const dragging = ref(false)

  function onDividerMouseDown(e: MouseEvent, index: number) {
    e.preventDefault()
    const tab = tabRef.value
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
    const startX = e.clientX
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

  return { dragging, onDividerMouseDown }
}

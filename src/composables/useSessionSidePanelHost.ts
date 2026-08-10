import { inject, nextTick, onBeforeUnmount, ref, watch, type ComputedRef, type Ref } from 'vue'
import { useWorkbench } from './useWorkbench'

interface SessionSidePanelHostOptions {
  rootRef: Ref<HTMLElement | undefined>
  close: () => void
}

const TRANSITION_MS = 250

export function useSessionSidePanelHost(
  visible: ComputedRef<boolean>,
  options: SessionSidePanelHostOptions,
) {
  const columnIndex = inject<ComputedRef<number> | undefined>('columnIndex', undefined)
  const tabId = inject<ComputedRef<string> | undefined>('tabId', undefined)
  const { state, activeTab } = useWorkbench()
  const mounted = ref(false)
  const expanded = ref(false)
  const targetWidth = ref(0)
  let transitionTimer: number | null = null
  let animationFrame: number | null = null

  function clearTransitionWork() {
    if (transitionTimer !== null) window.clearTimeout(transitionTimer)
    if (animationFrame !== null) window.cancelAnimationFrame(animationFrame)
    transitionTimer = null
    animationFrame = null
  }

  function columnContext() {
    const tab = tabId?.value
      ? state.value.tabs.find(item => item.id === tabId.value) ?? null
      : activeTab.value
    const index = columnIndex?.value
    if (!tab || index == null || index < 0 || index >= tab.columnSizes.length) return null
    return { tab, index }
  }

  function restoreColumnWidth() {
    const context = columnContext()
    if (!context || targetWidth.value <= 0) return
    const doubled = targetWidth.value * 2
    if (Math.abs(context.tab.columnSizes[context.index] - doubled) < 20) {
      context.tab.columnSizes[context.index] = targetWidth.value
    }
  }

  watch(visible, async (open) => {
    clearTransitionWork()
    const context = columnContext()
    if (open) {
      const rootWidth = options.rootRef.value?.clientWidth ?? 0
      targetWidth.value = context
        ? context.tab.columnSizes[context.index]
        : Math.min(448, Math.max(288, Math.round(rootWidth * 0.38) || 400))
      mounted.value = true
      await nextTick()
      animationFrame = window.requestAnimationFrame(() => {
        animationFrame = null
        expanded.value = true
        if (context) context.tab.columnSizes[context.index] = targetWidth.value * 2
        transitionTimer = window.setTimeout(() => {
          transitionTimer = null
          options.rootRef.value?.querySelector('.session-side-panel')
            ?.scrollIntoView({ inline: 'nearest', behavior: 'smooth' })
        }, TRANSITION_MS + 10)
      })
      return
    }

    expanded.value = false
    restoreColumnWidth()
    transitionTimer = window.setTimeout(() => {
      transitionTimer = null
      mounted.value = false
    }, TRANSITION_MS + 10)
  }, { immediate: true })

  watch(() => {
    const context = columnContext()
    return context?.tab.columnSizes[context.index]
  }, (columnWidth) => {
    if (!expanded.value || columnWidth == null) return
    if (columnWidth < targetWidth.value * 2 - 20) options.close()
  })

  onBeforeUnmount(() => {
    clearTransitionWork()
    restoreColumnWidth()
  })

  return { mounted, expanded, targetWidth }
}

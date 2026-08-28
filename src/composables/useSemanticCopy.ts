import { nextTick, onMounted, onUnmounted, ref, type Ref } from 'vue'
import {
  buildSemanticClipboardPayload,
  type SemanticCopyMode,
} from '@/utils/semanticClipboard'
import i18n from '@/locales'
import { useNotifications } from '@/composables/useNotifications'

interface CopyToolbarPosition {
  left: number
  top: number
}

let selectionPaintFrame = 0
let pointerSelectionSurface: HTMLElement | null = null

function sessionSurfaceOf(node: Node | null): HTMLElement | null {
  const element = node instanceof Element ? node : node?.parentElement
  return element?.closest<HTMLElement>('.session-surface') ?? null
}

function eventSessionSurface(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>('.session-surface') : null
}

function selectionSpansSessionSurfaces(selection: Selection): boolean {
  if (selection.isCollapsed || selection.rangeCount === 0) return false
  const anchorSurface = sessionSurfaceOf(selection.anchorNode)
  const focusSurface = sessionSurfaceOf(selection.focusNode)
  if (anchorSurface && focusSurface && anchorSurface !== focusSurface) return true

  const surfaces = document.querySelectorAll<HTMLElement>('.session-surface')
  const touchedSurfaces = new Set<HTMLElement>()
  for (let rangeIndex = 0; rangeIndex < selection.rangeCount; rangeIndex += 1) {
    const range = selection.getRangeAt(rangeIndex)
    const commonSurface = sessionSurfaceOf(range.commonAncestorContainer)
    if (commonSurface) {
      touchedSurfaces.add(commonSurface)
    } else {
      for (const surface of surfaces) {
        try {
          if (range.intersectsNode(surface)) touchedSurfaces.add(surface)
        } catch {
          // 流式更新可能让节点在检查期间脱离 DOM，下一次 selectionchange/pointerup 会重验。
        }
      }
    }
    if (touchedSurfaces.size > 1) return true
  }
  return false
}

/** WebKit 偶尔会在 Range 清空后保留 marker/空白区的选区绘制，强制全会话重绘一帧。 */
function repaintSelectionSurfaces() {
  const documentElement = document.documentElement
  documentElement.classList.add('selection-paint-reset')
  void documentElement.offsetHeight
  if (selectionPaintFrame) window.cancelAnimationFrame(selectionPaintFrame)
  selectionPaintFrame = window.requestAnimationFrame(() => {
    selectionPaintFrame = window.requestAnimationFrame(() => {
      selectionPaintFrame = 0
      documentElement.classList.remove('selection-paint-reset')
    })
  })
}

function removeNativeSelection(selection: Selection) {
  if (selection.rangeCount === 0) return
  try {
    // 折叠到真实 Range 末端，不能折叠到任意会话根节点，否则会把残影带到其他常驻会话。
    selection.collapseToEnd()
  } catch {
    // Range 可能在流式 DOM 更新中失效，removeAllRanges 仍可安全兜底。
  }
  selection.removeAllRanges()
  repaintSelectionSurfaces()
}

function selectionRangeWithin(root: HTMLElement): Range | null {
  const selection = window.getSelection()
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null
  const range = selection.getRangeAt(0)
  const ancestor = range.commonAncestorContainer
  const element = ancestor instanceof Element ? ancestor : ancestor.parentElement
  if (!element || !root.contains(element)) return null
  if (!element.closest('.session-viewport-scroll')) return null
  if (element.closest('input, textarea, [contenteditable="true"]')) return null
  return range.cloneRange()
}

export function useSemanticCopy(root: Ref<HTMLElement | null>) {
  const { notifyTransient } = useNotifications()
  const toolbarVisible = ref(false)
  const menuOpen = ref(false)
  const toolbarPosition = ref<CopyToolbarPosition>({ left: 0, top: 0 })
  let savedRange: Range | null = null
  let requestedMode: SemanticCopyMode | null = null
  let selectionFrame = 0

  function hideToolbar() {
    toolbarVisible.value = false
    menuOpen.value = false
  }

  function clearSelection() {
    const selection = window.getSelection()
    if (selection) removeNativeSelection(selection)
    savedRange = null
    requestedMode = null
    hideToolbar()
  }

  function updateSelection() {
    const selection = window.getSelection()
    if (selection && selectionSpansSessionSurfaces(selection)) {
      clearSelection()
      return
    }
    if (selectionFrame) window.cancelAnimationFrame(selectionFrame)
    selectionFrame = window.requestAnimationFrame(() => {
      selectionFrame = 0
      const surface = root.value
      if (!surface) return hideToolbar()
      const range = selectionRangeWithin(surface)
      if (!range || !range.toString().trim()) {
        savedRange = null
        requestedMode = null
        return hideToolbar()
      }
      savedRange = range
      const rect = range.getBoundingClientRect()
      const width = 144
      toolbarPosition.value = {
        left: Math.min(Math.max(8, rect.left + rect.width / 2 - width / 2), window.innerWidth - width - 8),
        top: Math.max(8, rect.top - 38),
      }
      toolbarVisible.value = true
    })
  }

  function writePayload(event: ClipboardEvent, mode: SemanticCopyMode, range: Range): boolean {
    if (!event.clipboardData) return false
    const payload = buildSemanticClipboardPayload(range.cloneContents(), mode)
    event.preventDefault()
    event.clipboardData.clearData()
    event.clipboardData.setData('text/plain', payload.plain)
    if (payload.markdown !== undefined) event.clipboardData.setData('text/markdown', payload.markdown)
    if (payload.html !== undefined) event.clipboardData.setData('text/html', payload.html)
    return true
  }

  function onCopy(event: ClipboardEvent) {
    const surface = root.value
    if (!surface) return
    const explicitMode = requestedMode
    const range = explicitMode ? savedRange : selectionRangeWithin(surface)
    if (!range || !surface.contains(range.commonAncestorContainer)) return
    const mode = explicitMode ?? 'rich'
    requestedMode = null
    try {
      if (writePayload(event, mode, range)) {
        if (explicitMode) clearSelection()
        else hideToolbar()
      }
    } catch (cause) {
      requestedMode = null
      notifyTransient(i18n.global.t('copy.actionFailed'), String(cause))
    }
  }

  async function copy(mode: SemanticCopyMode) {
    if (!savedRange) return
    const range = savedRange.cloneRange()
    const selection = window.getSelection()
    if (!selection) return
    selection.removeAllRanges()
    selection.addRange(range)
    requestedMode = mode
    let copied = false
    try {
      copied = document.execCommand('copy')
    } catch {
      copied = false
    }
    if (copied) clearSelection()
    if (!copied) {
      requestedMode = null
      try {
        const payload = buildSemanticClipboardPayload(range.cloneContents(), mode)
        if (mode === 'markdown') {
          await navigator.clipboard.writeText(payload.markdown ?? payload.plain)
        } else {
          const values: Record<string, Blob> = {
            'text/plain': new Blob([payload.plain], { type: 'text/plain' }),
          }
          if (payload.html !== undefined) values['text/html'] = new Blob([payload.html], { type: 'text/html' })
          if (payload.markdown !== undefined) values['text/markdown'] = new Blob([payload.markdown], { type: 'text/markdown' })
          try {
            await navigator.clipboard.write([new ClipboardItem(values)])
          } catch (cause) {
            if (payload.markdown === undefined) throw cause
            delete values['text/markdown']
            await navigator.clipboard.write([new ClipboardItem(values)])
          }
        }
        clearSelection()
      } catch (cause) {
        notifyTransient(i18n.global.t('copy.actionFailed'), String(cause))
      }
    }
    await nextTick()
  }

  function toggleMenu() {
    menuOpen.value = !menuOpen.value
  }

  function onPointerDown(event: PointerEvent) {
    pointerSelectionSurface = event.button === 0 ? eventSessionSurface(event.target) : null
    const selection = window.getSelection()
    if (event.button !== 0 || !selection || selection.isCollapsed || selection.rangeCount === 0) return
    const target = event.target
    if (target instanceof Element && target.closest('[data-copy-exclude]')) return
    clearSelection()
  }

  function onPointerMove(event: PointerEvent) {
    if ((event.buttons & 1) === 0 || !pointerSelectionSurface) return
    if (eventSessionSurface(event.target) === pointerSelectionSurface) return
    event.preventDefault()
    clearSelection()
  }

  function onKeyDown(event: KeyboardEvent) {
    const selection = window.getSelection()
    if (event.key === 'Escape' && selection && !selection.isCollapsed && selection.rangeCount > 0) {
      clearSelection()
    }
  }

  function onPointerUp(event: PointerEvent) {
    if (event.button !== 0) return
    const selection = window.getSelection()
    const crossedBoundary = !!pointerSelectionSurface
      && eventSessionSurface(event.target) !== pointerSelectionSurface
    pointerSelectionSurface = null
    if (crossedBoundary || (selection && selectionSpansSessionSurfaces(selection))) clearSelection()
  }

  onMounted(() => {
    document.addEventListener('selectionchange', updateSelection)
    document.addEventListener('copy', onCopy, true)
    document.addEventListener('pointerdown', onPointerDown, true)
    document.addEventListener('pointermove', onPointerMove, true)
    document.addEventListener('pointerup', onPointerUp, true)
    document.addEventListener('keydown', onKeyDown, true)
    window.addEventListener('resize', updateSelection)
    window.addEventListener('scroll', updateSelection, true)
  })

  onUnmounted(() => {
    if (selectionFrame) window.cancelAnimationFrame(selectionFrame)
    document.removeEventListener('selectionchange', updateSelection)
    document.removeEventListener('copy', onCopy, true)
    document.removeEventListener('pointerdown', onPointerDown, true)
    document.removeEventListener('pointermove', onPointerMove, true)
    document.removeEventListener('pointerup', onPointerUp, true)
    document.removeEventListener('keydown', onKeyDown, true)
    window.removeEventListener('resize', updateSelection)
    window.removeEventListener('scroll', updateSelection, true)
  })

  return {
    toolbarVisible,
    menuOpen,
    toolbarPosition,
    copy,
    toggleMenu,
  }
}

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

  function updateSelection() {
    if (selectionFrame) window.cancelAnimationFrame(selectionFrame)
    selectionFrame = window.requestAnimationFrame(() => {
      selectionFrame = 0
      const surface = root.value
      if (!surface) return hideToolbar()
      const range = selectionRangeWithin(surface)
      if (!range || !range.toString().trim()) return hideToolbar()
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
    const range = requestedMode ? savedRange : selectionRangeWithin(surface)
    if (!range || !surface.contains(range.commonAncestorContainer)) return
    const mode = requestedMode ?? 'rich'
    requestedMode = null
    try {
      if (writePayload(event, mode, range)) hideToolbar()
    } catch (cause) {
      requestedMode = null
      notifyTransient(i18n.global.t('copy.actionFailed'), String(cause))
    }
  }

  async function copy(mode: SemanticCopyMode) {
    if (!savedRange) return
    const selection = window.getSelection()
    if (!selection) return
    selection.removeAllRanges()
    selection.addRange(savedRange)
    requestedMode = mode
    let copied = false
    try {
      copied = document.execCommand('copy')
    } catch {
      copied = false
    }
    if (!copied) {
      requestedMode = null
      try {
        const payload = buildSemanticClipboardPayload(savedRange.cloneContents(), mode)
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
        hideToolbar()
      } catch (cause) {
        notifyTransient(i18n.global.t('copy.actionFailed'), String(cause))
      }
    }
    await nextTick()
  }

  function toggleMenu() {
    menuOpen.value = !menuOpen.value
  }

  onMounted(() => {
    document.addEventListener('selectionchange', updateSelection)
    document.addEventListener('copy', onCopy, true)
    window.addEventListener('resize', updateSelection)
    window.addEventListener('scroll', updateSelection, true)
  })

  onUnmounted(() => {
    if (selectionFrame) window.cancelAnimationFrame(selectionFrame)
    document.removeEventListener('selectionchange', updateSelection)
    document.removeEventListener('copy', onCopy, true)
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

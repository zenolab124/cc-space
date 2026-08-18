import { readonly, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import html2canvas from 'html2canvas-pro'
import i18n from '@/locales'
import { useNotifications } from '@/composables/useNotifications'
import { useWorkbench } from '@/composables/useWorkbench'
import {
  calculatePanoramaLayout,
  calculateNativeCaptureOffsets,
  workbenchCaptureFilename,
  type WorkbenchCaptureMode,
} from '@/utils/workbenchCapture'

const isCapturing = ref(false)
const ICON_RASTER_SCALE = 2

function nextFrame(): Promise<void> {
  return new Promise(resolve => requestAnimationFrame(() => resolve()))
}

async function blobToBase64(blob: Blob): Promise<string> {
  return await new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(reader.error ?? new Error('Could not read capture data'))
    reader.onload = () => {
      const result = String(reader.result ?? '')
      const comma = result.indexOf(',')
      if (comma < 0) reject(new Error('Could not encode capture data'))
      else resolve(result.slice(comma + 1))
    }
    reader.readAsDataURL(blob)
  })
}

async function canvasToBlob(canvas: HTMLCanvasElement): Promise<Blob> {
  return await new Promise((resolve, reject) => {
    canvas.toBlob(blob => {
      if (blob) resolve(blob)
      else reject(new Error(i18n.global.t('workbench.capture.renderFailed')))
    }, 'image/png')
  })
}

function svgFromCssUrl(cssUrl: string): string | null {
  const match = cssUrl.trim().match(/^url\((["']?)(data:image\/svg\+xml[^,]*,.*)\1\)$/)
  if (!match) return null
  const dataUrl = match[2]
  const comma = dataUrl.indexOf(',')
  if (comma < 0) return null
  const metadata = dataUrl.slice(0, comma)
  const payload = dataUrl.slice(comma + 1)
  try {
    if (!metadata.includes(';base64')) return decodeURIComponent(payload)
    const bytes = Uint8Array.from(atob(payload), character => character.charCodeAt(0))
    return new TextDecoder().decode(bytes)
  } catch {
    return null
  }
}

function rasterizeMaskedIcon(source: HTMLElement, clone: HTMLElement): void {
  const style = getComputedStyle(source)
  const svgSource = svgFromCssUrl(style.getPropertyValue('--un-icon'))
  if (!svgSource) return

  const document = new DOMParser().parseFromString(svgSource, 'image/svg+xml')
  const svg = document.documentElement
  const viewBox = svg.getAttribute('viewBox')?.trim().split(/\s+/).map(Number)
  const rect = source.getBoundingClientRect()
  if (!viewBox || viewBox.length !== 4 || viewBox.some(Number.isNaN) || rect.width <= 0 || rect.height <= 0) return

  const canvas = window.document.createElement('canvas')
  canvas.width = Math.ceil(rect.width * ICON_RASTER_SCALE)
  canvas.height = Math.ceil(rect.height * ICON_RASTER_SCALE)
  canvas.style.width = `${rect.width}px`
  canvas.style.height = `${rect.height}px`
  canvas.style.display = 'block'

  const context = canvas.getContext('2d')
  if (!context) return
  const [minX, minY, width, height] = viewBox
  context.scale(canvas.width / width, canvas.height / height)
  context.translate(-minX, -minY)
  context.fillStyle = style.color
  context.strokeStyle = style.color

  for (const pathElement of document.querySelectorAll('path')) {
    const pathData = pathElement.getAttribute('d')
    if (!pathData) continue
    const path = new Path2D(pathData)
    if (pathElement.getAttribute('fill') !== 'none') {
      const fillRule = pathElement.getAttribute('fill-rule') === 'evenodd' ? 'evenodd' : 'nonzero'
      context.fill(path, fillRule)
    }
    if (pathElement.getAttribute('stroke') && pathElement.getAttribute('stroke') !== 'none') {
      context.lineWidth = Number(pathElement.getAttribute('stroke-width')) || 1
      context.stroke(path)
    }
  }

  clone.style.setProperty('background-color', 'transparent', 'important')
  clone.style.setProperty('-webkit-mask', 'none', 'important')
  clone.style.setProperty('mask', 'none', 'important')
  clone.replaceChildren(canvas)
}

function copyRuntimeState(source: HTMLElement, clone: HTMLElement): void {
  const sourceElements = [source, ...source.querySelectorAll<HTMLElement>('*')]
  const cloneElements = [clone, ...clone.querySelectorAll<HTMLElement>('*')]
  for (let index = 0; index < sourceElements.length; index++) {
    const sourceElement = sourceElements[index]
    const cloneElement = cloneElements[index]
    if (!cloneElement) continue

    if (sourceElement instanceof HTMLInputElement && cloneElement instanceof HTMLInputElement) {
      cloneElement.value = sourceElement.value
      cloneElement.checked = sourceElement.checked
    } else if (sourceElement instanceof HTMLTextAreaElement && cloneElement instanceof HTMLTextAreaElement) {
      cloneElement.value = sourceElement.value
      cloneElement.textContent = sourceElement.value
    } else if (sourceElement instanceof HTMLSelectElement && cloneElement instanceof HTMLSelectElement) {
      cloneElement.value = sourceElement.value
    } else if (sourceElement instanceof HTMLDetailsElement && cloneElement instanceof HTMLDetailsElement) {
      cloneElement.open = sourceElement.open
    } else if (sourceElement instanceof HTMLCanvasElement && cloneElement instanceof HTMLCanvasElement) {
      cloneElement.width = sourceElement.width
      cloneElement.height = sourceElement.height
      cloneElement.getContext('2d')?.drawImage(sourceElement, 0, 0)
    }

    rasterizeMaskedIcon(sourceElement, cloneElement)

    if (sourceElement.hasAttribute('data-workbench-panorama')) continue
    const offsetX = sourceElement.scrollLeft
    const offsetY = sourceElement.scrollTop
    if (offsetX === 0 && offsetY === 0) continue
    cloneElement.style.overflow = 'hidden'
    for (const child of cloneElement.children) {
      if (!(child instanceof HTMLElement)) continue
      child.style.translate = `${-offsetX}px ${-offsetY}px`
    }
  }
}

function createCaptureSurface(source: HTMLElement, width: number, height: number): {
  root: HTMLElement
  dispose: () => void
} {
  const wrapper = document.createElement('div')
  wrapper.setAttribute('aria-hidden', 'true')
  Object.assign(wrapper.style, {
    position: 'fixed',
    left: '-100000px',
    top: '0',
    width: `${width}px`,
    height: `${height}px`,
    overflow: 'hidden',
    pointerEvents: 'none',
  })

  const root = source.cloneNode(true) as HTMLElement
  root.classList.add('workbench-capture-active')
  root.style.width = `${width}px`
  root.style.height = `${height}px`
  root.style.position = 'relative'
  root.style.inset = 'auto'
  copyRuntimeState(source, root)
  root.querySelectorAll('[data-capture-exclude]').forEach(element => element.remove())
  root.querySelectorAll<HTMLElement>('[data-workbench-panorama]').forEach(element => {
    element.scrollLeft = 0
  })

  wrapper.appendChild(root)
  document.body.appendChild(wrapper)
  return {
    root,
    dispose: () => wrapper.remove(),
  }
}

async function renderExpandedSurface(
  root: HTMLElement,
  layout: ReturnType<typeof calculatePanoramaLayout>,
): Promise<HTMLCanvasElement> {
  const surface = createCaptureSurface(root, layout.width, layout.height)

  try {
    await document.fonts?.ready
    await nextFrame()
    await nextFrame()

    return await html2canvas(surface.root, {
      width: layout.width,
      height: layout.height,
      scale: layout.pixelRatio,
      backgroundColor: getComputedStyle(surface.root).backgroundColor,
      foreignObjectRendering: false,
      ignoreElements: element => element.hasAttribute('data-capture-exclude'),
      imageSmoothing: true,
      imageSmoothingQuality: 'high',
      logging: false,
      scrollX: 0,
      scrollY: 0,
      useCORS: true,
      windowWidth: layout.width,
      windowHeight: layout.height,
    })
  } finally {
    surface.dispose()
  }
}

async function renderPanorama(): Promise<Blob> {
  const root = document.querySelector<HTMLElement>('[data-workbench-capture-root]')
  const scroller = document.querySelector<HTMLElement>('[data-workbench-panorama]')
  if (!root || !scroller) throw new Error(i18n.global.t('workbench.capture.unavailable'))

  const rootRect = root.getBoundingClientRect()
  const layout = calculatePanoramaLayout(
    rootRect.width,
    rootRect.height,
    scroller.clientWidth,
    scroller.scrollWidth,
  )
  return await canvasToBlob(await renderExpandedSurface(root, layout))
}

interface NativeCaptureTile {
  image: HTMLImageElement
  scrollLeft: number
}

async function loadPngImage(pngBase64: string): Promise<HTMLImageElement> {
  return await new Promise((resolve, reject) => {
    const image = new Image()
    image.onload = () => resolve(image)
    image.onerror = () => reject(new Error(i18n.global.t('workbench.capture.renderFailed')))
    image.src = `data:image/png;base64,${pngBase64}`
  })
}

async function renderNativePanorama(): Promise<Blob> {
  const root = document.querySelector<HTMLElement>('[data-workbench-capture-root]')
  const scroller = document.querySelector<HTMLElement>('[data-workbench-panorama]')
  if (!root || !scroller) throw new Error(i18n.global.t('workbench.capture.unavailable'))

  const rootRect = root.getBoundingClientRect()
  const layout = calculatePanoramaLayout(
    rootRect.width,
    rootRect.height,
    scroller.clientWidth,
    scroller.scrollWidth,
  )
  const scrollerRect = scroller.getBoundingClientRect()
  const scrollerX = scrollerRect.left - rootRect.left
  const scrollerY = scrollerRect.top - rootRect.top
  const originalScrollLeft = scroller.scrollLeft
  const tiles: NativeCaptureTile[] = []
  const atmosphere = document.body

  // 原生 tile 只覆盖横向滚动的列区。先以全景宽度重排并渲染完整外壳，
  // 让标题栏与底部输入区自然延伸到最终宽度；随后再用 WebKit tile
  // 覆盖列区，保留 iframe 等原生渲染内容。
  const expandedSurface = await renderExpandedSurface(root, layout)

  root.classList.add('workbench-native-capture-active')
  atmosphere.classList.add('workbench-native-atmosphere-active')
  atmosphere.style.setProperty('--workbench-native-atmosphere-width', `${layout.width}px`)
  try {
    await document.fonts?.ready
    for (const offset of calculateNativeCaptureOffsets(scroller.clientWidth, scroller.scrollWidth)) {
      scroller.scrollLeft = offset
      const scrollLeft = scroller.scrollLeft
      atmosphere.style.setProperty('--workbench-native-atmosphere-left', `${-scrollLeft}px`)
      await nextFrame()
      await nextFrame()
      if (tiles.some(tile => tile.scrollLeft === scrollLeft)) continue
      const pngBase64 = await invoke<string>('capture_native_workbench_tile')
      tiles.push({ image: await loadPngImage(pngBase64), scrollLeft })
    }
  } finally {
    scroller.scrollLeft = originalScrollLeft
    root.classList.remove('workbench-native-capture-active')
    atmosphere.classList.remove('workbench-native-atmosphere-active')
    atmosphere.style.removeProperty('--workbench-native-atmosphere-width')
    atmosphere.style.removeProperty('--workbench-native-atmosphere-left')
    await nextFrame()
  }
  if (tiles.length === 0) throw new Error(i18n.global.t('workbench.capture.renderFailed'))

  const nativeScale = tiles[0].image.naturalWidth / rootRect.width
  const outputScale = Math.min(nativeScale, layout.pixelRatio)
  const canvas = document.createElement('canvas')
  canvas.width = Math.round(layout.width * outputScale)
  canvas.height = Math.round(layout.height * outputScale)
  const context = canvas.getContext('2d')
  if (!context) throw new Error(i18n.global.t('workbench.capture.renderFailed'))
  context.imageSmoothingEnabled = true
  context.imageSmoothingQuality = 'high'

  context.drawImage(
    expandedSurface,
    0,
    0,
    expandedSurface.width,
    expandedSurface.height,
    0,
    0,
    canvas.width,
    canvas.height,
  )

  for (const tile of tiles) {
    context.drawImage(
      tile.image,
      scrollerX * nativeScale,
      scrollerY * nativeScale,
      scrollerRect.width * nativeScale,
      scrollerRect.height * nativeScale,
      (scrollerX + tile.scrollLeft) * outputScale,
      scrollerY * outputScale,
      scrollerRect.width * outputScale,
      scrollerRect.height * outputScale,
    )
  }
  return await canvasToBlob(canvas)
}

export function useWorkbenchCapture() {
  const { activeTab } = useWorkbench()
  const { notifyTransient } = useNotifications()

  async function captureWorkbench(mode: WorkbenchCaptureMode = 'native'): Promise<void> {
    if (isCapturing.value || activeTab.value.columns.length === 0) return
    const path = await save({
      defaultPath: workbenchCaptureFilename(activeTab.value.name, mode),
      filters: [{ name: 'PNG', extensions: ['png'] }],
    })
    if (!path) return

    isCapturing.value = true
    try {
      if (mode === 'native') {
        const blob = await renderNativePanorama()
        await invoke('save_workbench_capture', {
          path,
          pngBase64: await blobToBase64(blob),
        })
      } else {
        const blob = await renderPanorama()
        await invoke('save_workbench_capture', {
          path,
          pngBase64: await blobToBase64(blob),
        })
      }
      notifyTransient(i18n.global.t('workbench.capture.saved'))
    } catch (cause) {
      const detail = cause instanceof Error ? cause.message : String(cause)
      notifyTransient(i18n.global.t('workbench.capture.failed'), detail)
    } finally {
      isCapturing.value = false
    }
  }

  return {
    isCapturing: readonly(isCapturing),
    captureWorkbench,
  }
}

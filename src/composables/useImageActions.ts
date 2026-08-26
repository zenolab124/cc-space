import { invoke } from '@tauri-apps/api/core'
import { Menu } from '@tauri-apps/api/menu'
import i18n from '@/locales'
import { useNotifications } from '@/composables/useNotifications'
import { openExternalUrl } from '@/composables/useFileOpener'

export interface ImageActionSource {
  src?: string | null
  url?: string | null
  path?: string | null
  fileRoot?: string | null
}

function stableRemoteUrl(value?: string | null): string | null {
  const candidate = value?.trim() ?? ''
  if (!/^https?:\/\//i.test(candidate)) return null
  if (/^https?:\/\/(?:ccimg|asset)\.localhost(?:\/|$)/i.test(candidate)) return null
  return candidate
}

function isAbsolutePath(value: string): boolean {
  return value.startsWith('/') || value.startsWith('\\\\') || /^[a-z]:[\\/]/i.test(value)
}

function normalizeSegments(path: string, separator: '/' | '\\'): string {
  const unixPrefix = separator === '/' && path.startsWith('/') ? '/' : ''
  const uncPrefix = separator === '\\' && path.startsWith('\\\\') ? '\\\\' : ''
  const windowsPrefix = separator === '\\' && /^[a-z]:/i.test(path) ? path.slice(0, 2) : ''
  const body = windowsPrefix ? path.slice(2) : path
  const parts: string[] = []
  for (const part of body.split(/[\\/]+/)) {
    if (!part || part === '.') continue
    if (part === '..') parts.pop()
    else parts.push(part)
  }
  if (windowsPrefix) return `${windowsPrefix}${separator}${parts.join(separator)}`
  return `${uncPrefix || unixPrefix}${parts.join(separator)}`
}

export function resolveImagePath(value?: string | null, fileRoot?: string | null): string | null {
  let candidate = value?.trim() ?? ''
  if (!candidate || /^(?:data|blob|ccimg):/i.test(candidate) || /^https?:/i.test(candidate)) return null
  if (/^file:/i.test(candidate)) {
    try {
      candidate = decodeURIComponent(new URL(candidate).pathname)
      if (/^\/[a-z]:\//i.test(candidate)) candidate = candidate.slice(1).replaceAll('/', '\\')
    } catch {
      return null
    }
  } else {
    if (/^[a-z][a-z\d+.-]*:/i.test(candidate) && !/^[a-z]:[\\/]/i.test(candidate)) return null
    try {
      candidate = decodeURIComponent(candidate.split(/[?#]/, 1)[0] ?? candidate)
    } catch {
      return null
    }
  }
  if (isAbsolutePath(candidate)) {
    return normalizeSegments(candidate, candidate.includes('\\') ? '\\' : '/')
  }
  const root = fileRoot?.trim()
  if (!root) return null
  const separator = root.includes('\\') ? '\\' : '/'
  return normalizeSegments(`${root}${separator}${candidate}`, separator)
}

async function blobFromLocalPath(path: string): Promise<Blob> {
  const result = await invoke<{ data: string; mime_type: string }>('read_local_image', { path })
  const binary = atob(result.data)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index)
  return new Blob([bytes], { type: result.mime_type })
}

async function pngBlob(source: Blob): Promise<Blob> {
  if (source.type === 'image/png') return source
  const objectUrl = URL.createObjectURL(source)
  try {
    const image = await new Promise<HTMLImageElement>((resolve, reject) => {
      const element = new Image()
      element.onload = () => resolve(element)
      element.onerror = () => reject(new Error(i18n.global.t('copy.imageDecodeFailed')))
      element.src = objectUrl
    })
    const canvas = document.createElement('canvas')
    canvas.width = image.naturalWidth
    canvas.height = image.naturalHeight
    const context = canvas.getContext('2d')
    if (!context) throw new Error(i18n.global.t('copy.imageDecodeFailed'))
    context.drawImage(image, 0, 0)
    return await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob(blob => blob ? resolve(blob) : reject(new Error(i18n.global.t('copy.imageDecodeFailed'))), 'image/png')
    })
  } finally {
    URL.revokeObjectURL(objectUrl)
  }
}

async function copyImage(source: ImageActionSource, path: string | null) {
  if (!path && !source.src) throw new Error(i18n.global.t('copy.imageUnavailable'))
  const blob = path
    ? await blobFromLocalPath(path)
    : await fetch(source.src ?? '').then(response => {
        if (!response.ok) throw new Error(`${response.status} ${response.statusText}`)
        return response.blob()
      })
  const png = await pngBlob(blob)
  await navigator.clipboard.write([new ClipboardItem({ 'image/png': png })])
}

function imageSourceFromTarget(image: HTMLImageElement, fileRoot?: string | null): ImageActionSource {
  return {
    src: image.currentSrc || image.src,
    url: image.dataset.imageUrl || image.getAttribute('src'),
    path: image.dataset.imagePath,
    fileRoot,
  }
}

export async function showImageContextMenu(event: MouseEvent, source: ImageActionSource): Promise<void> {
  event.preventDefault()
  event.stopPropagation()
  const { notifyTransient } = useNotifications()
  const remoteUrl = stableRemoteUrl(source.url)
  const localPath = resolveImagePath(source.path ?? source.url, source.fileRoot)
  const perform = (action: () => Promise<unknown>, success: string) => {
    void action()
      .then(() => notifyTransient(success))
      .catch(cause => notifyTransient(i18n.global.t('copy.actionFailed'), String(cause)))
  }
  const items: Array<{ text: string; action: () => void }> = [
    {
      text: i18n.global.t('copy.image'),
      action: () => perform(() => copyImage(source, localPath), i18n.global.t('copy.imageCopied')),
    },
  ]
  if (remoteUrl) {
    items.push(
      {
        text: i18n.global.t('copy.imageAddress'),
        action: () => perform(() => navigator.clipboard.writeText(remoteUrl), i18n.global.t('copy.addressCopied')),
      },
      {
        text: i18n.global.t('copy.openImage'),
        action: () => perform(() => openExternalUrl(remoteUrl), i18n.global.t('copy.imageOpened')),
      },
    )
  } else if (localPath) {
    items.push(
      {
        text: i18n.global.t('copy.filePath'),
        action: () => perform(() => navigator.clipboard.writeText(localPath), i18n.global.t('copy.pathCopied')),
      },
      {
        text: i18n.global.t('common.revealInFileManager'),
        action: () => perform(() => invoke('reveal_in_finder', { path: localPath }), i18n.global.t('copy.locationOpened')),
      },
    )
  }
  const menu = await Menu.new({ items })
  await menu.popup()
}

export function onSessionImageContextMenu(event: MouseEvent, fileRoot?: string | null): void {
  const target = event.target
  if (!(target instanceof Element)) return
  const image = target.closest<HTMLImageElement>('img')
  if (!image || !image.closest('.session-viewport-scroll')) return
  void showImageContextMenu(event, imageSourceFromTarget(image, fileRoot))
}

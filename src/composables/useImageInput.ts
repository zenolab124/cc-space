/**
 * 输入框图片支持 (FR-005):
 * - 粘贴 (Cmd+V) / 拖拽监听
 * - 校验 + 压缩
 * - 缩略图状态管理
 * - 发送时把图片转换为 image block 序列
 *
 * PRD: docs/prd/v1.0.0-realtime-session-experience.md FR-005
 */

import { ref, computed, watch, onBeforeUnmount, type Ref } from 'vue'
import i18n from '../locales'
import {
  validateImage,
  readMagicProbe,
  type AllowedMime,
} from '@/utils/imageValidate'
import {
  compressIfNeeded,
  blobToBase64,
  blobToDataUrl,
} from '@/utils/imageCompress'

/** @deprecated 不再限制单条消息图片数 */
export const MAX_IMAGES_PER_MESSAGE = Infinity

/** 输入框暂存的图片(已校验并按需压缩) */
export interface PendingImage {
  /** 唯一 id,用于列表 key 和删除 */
  id: string
  /** 用于缩略图的 data URL(仅前端显示) */
  dataUrl: string
  /** 已校验/压缩后的 MIME */
  mime: AllowedMime | string
  /** 字节数 */
  size: number
  /** 压缩后的 Blob,发送时转 base64 */
  blob: Blob
}

/**
 * Anthropic Messages API 风格的 image content block
 * 用于方案 a (stdin stream-json) 直接拼到 user message content 数组
 */
export interface ImageContentBlock {
  type: 'image'
  source: {
    type: 'base64'
    media_type: AllowedMime | string
    data: string
  }
}

/**
 * 错误类型枚举(供 UI 展示分类用)
 */
export type ImageInputErrorKind =
  | 'validate' // 校验失败
  | 'compress' // 压缩失败或仍超限
  | 'limit' // 数量超限
  | 'read' // FileReader / ImageBitmap 失败

export interface ImageInputError {
  kind: ImageInputErrorKind
  message: string
}

/**
 * useImageInput 配置项
 */
export interface UseImageInputOptions {
  /**
   * 输入区根元素 ref(用于拖拽事件绑定);
   * 可选:为 null 时由调用方自行处理拖拽,只用本 composable 的状态管理
   */
  dropTarget?: Ref<HTMLElement | null | undefined>
  /**
   * 文本输入框 ref(用于绑定粘贴事件);
   * 粘贴事件必须绑在 textarea 上才能在聚焦时触发,符合 PRD"输入框失焦时不接受粘贴"
   */
  pasteTarget?: Ref<HTMLTextAreaElement | HTMLInputElement | null | undefined>
}

let _idSeq = 0
function nextId(): string {
  _idSeq += 1
  return `img-${Date.now().toString(36)}-${_idSeq}`
}

/**
 * 主入口
 */
export function useImageInput(opts: UseImageInputOptions = {}) {
  /** 暂存图片队列(按用户添加顺序) */
  const images = ref<PendingImage[]>([])

  /** 最近一次错误(单条,UI 展示后由调用方决定是否清空) */
  const lastError = ref<ImageInputError | null>(null)

  /** 拖拽进入态(用于显示蓝色高亮提示) */
  const isDragging = ref(false)

  const canAddMore = computed(() => true)
  const remainingSlots = computed(() => Infinity)

  /**
   * 添加单个文件(File 或 Blob 均可)
   * 流程:校验 → 压缩 → 转 dataUrl → 入队
   * 失败时设置 lastError,不抛异常
   */
  async function addFile(file: File | Blob): Promise<boolean> {

    const hintedMime = (file as File).type ?? ''

    let probeBytes: Uint8Array
    try {
      probeBytes = await readMagicProbe(file)
    } catch (e) {
      lastError.value = {
        kind: 'read',
        message: i18n.global.t('image.readFailed', { error: (e as Error).message || String(e) }),
      }
      return false
    }

    // 校验
    const validated = validateImage(hintedMime, probeBytes)
    if (!validated.ok || !validated.mime) {
      lastError.value = {
        kind: 'validate',
        message: validated.reason ?? i18n.global.t('image.unsupportedType'),
      }
      return false
    }

    // 压缩(>5MB 走 canvas,quality=80)
    const compressed = await compressIfNeeded(file, validated.mime)
    if (!compressed.ok || !compressed.blob) {
      lastError.value = {
        kind: 'compress',
        message: compressed.reason ?? i18n.global.t('image.processFailed'),
      }
      return false
    }

    // 转 data URL 用于缩略图
    let dataUrl: string
    try {
      dataUrl = await blobToDataUrl(compressed.blob)
    } catch (e) {
      lastError.value = {
        kind: 'read',
        message: i18n.global.t('image.thumbnailFailed', { error: (e as Error).message || String(e) }),
      }
      return false
    }

    images.value.push({
      id: nextId(),
      dataUrl,
      mime: compressed.mime,
      size: compressed.blob.size,
      blob: compressed.blob,
    })
    return true
  }

  /** 批量添加(粘贴板/拖拽常会一次给多个);超出额度只接受前 N 张并报错一次 */
  async function addFiles(files: ArrayLike<File | Blob>): Promise<{
    added: number
    rejected: number
  }> {
    let added = 0
    let rejected = 0
    const acceptable = Array.from(files)
    for (const f of acceptable) {
      const ok = await addFile(f)
      if (ok) added += 1
      else rejected += 1
    }
    return { added, rejected }
  }

  /** 删除指定 id 的图片 */
  function removeImage(id: string) {
    const idx = images.value.findIndex(i => i.id === id)
    if (idx >= 0) images.value.splice(idx, 1)
  }

  /** 清空(发送后调用) */
  function clearImages() {
    images.value = []
    lastError.value = null
  }

  /** 清空错误提示(用户开始新输入时调用) */
  function clearError() {
    lastError.value = null
  }

  /**
   * 把暂存图片序列化为 Anthropic image content block 数组。
   * 供方案 a 使用:与 text block 一起拼到 user message.content。
   */
  async function toImageBlocks(sourceImages: readonly PendingImage[] = images.value): Promise<ImageContentBlock[]> {
    const blocks: ImageContentBlock[] = []
    for (const img of sourceImages) {
      const data = await blobToBase64(img.blob)
      blocks.push({
        type: 'image',
        source: { type: 'base64', media_type: img.mime, data },
      })
    }
    return blocks
  }

  // ---- 事件绑定:粘贴 ----
  function onPaste(e: ClipboardEvent) {
    const items = e.clipboardData?.items
    const itemDetails = items
      ? Array.from(items).map((it, i) => `[${i}] kind=${it.kind} type=${it.type}`)
      : []
    console.debug('[ImagePaste] clipboardData.items:', items?.length ?? 'null', itemDetails)
    if (!items || items.length === 0) return
    const files: File[] = []
    for (let i = 0; i < items.length; i += 1) {
      const item = items[i]
      if (item.kind === 'file') {
        const f = item.getAsFile()
        if (f) files.push(f)
      }
    }
    console.debug('[ImagePaste] extracted files:', files.length, files.map(f => `${f.type} ${f.size}B`))
    if (files.length === 0) return
    // 命中图片粘贴,阻止默认行为(否则文本框可能插入文件名)
    e.preventDefault()
    void addFiles(files)
  }

  // ---- 事件绑定:拖拽 ----
  function onDragEnter(e: DragEvent) {
    if (!hasFiles(e)) return
    e.preventDefault()
    isDragging.value = true
  }

  function onDragOver(e: DragEvent) {
    if (!hasFiles(e)) return
    e.preventDefault()
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'
    isDragging.value = true
  }

  function onDragLeave(e: DragEvent) {
    // 只有真正离开容器时才置 false(子元素冒泡的 leave 由 relatedTarget 判定)
    const root = opts.dropTarget?.value
    if (root && e.relatedTarget && root.contains(e.relatedTarget as Node)) {
      return
    }
    isDragging.value = false
  }

  function onDrop(e: DragEvent) {
    if (!hasFiles(e)) return
    e.preventDefault()
    isDragging.value = false
    const dt = e.dataTransfer
    if (!dt) return
    const files = Array.from(dt.files)
    if (files.length === 0) return
    void addFiles(files)
  }

  function hasFiles(e: DragEvent): boolean {
    const types = e.dataTransfer?.types
    if (!types) return false
    return Array.prototype.includes.call(types, 'Files')
  }

  // ---- 自动绑定/解绑(opts 提供时) ----
  let pasteEl: HTMLElement | null = null
  let dropEl: HTMLElement | null = null

  function attach() {
    const p = opts.pasteTarget?.value
    if (p && p !== pasteEl) {
      if (pasteEl) pasteEl.removeEventListener('paste', onPaste as EventListener)
      pasteEl = p as HTMLElement
      pasteEl.addEventListener('paste', onPaste as EventListener)
    }
    const d = opts.dropTarget?.value
    if (d && d !== dropEl) {
      if (dropEl) {
        dropEl.removeEventListener('dragenter', onDragEnter as EventListener)
        dropEl.removeEventListener('dragover', onDragOver as EventListener)
        dropEl.removeEventListener('dragleave', onDragLeave as EventListener)
        dropEl.removeEventListener('drop', onDrop as EventListener)
      }
      dropEl = d
      dropEl.addEventListener('dragenter', onDragEnter as EventListener)
      dropEl.addEventListener('dragover', onDragOver as EventListener)
      dropEl.addEventListener('dragleave', onDragLeave as EventListener)
      dropEl.addEventListener('drop', onDrop as EventListener)
    }
  }

  function detach() {
    if (pasteEl) {
      pasteEl.removeEventListener('paste', onPaste as EventListener)
      pasteEl = null
    }
    if (dropEl) {
      dropEl.removeEventListener('dragenter', onDragEnter as EventListener)
      dropEl.removeEventListener('dragover', onDragOver as EventListener)
      dropEl.removeEventListener('dragleave', onDragLeave as EventListener)
      dropEl.removeEventListener('drop', onDrop as EventListener)
      dropEl = null
    }
  }

  if (opts.pasteTarget) {
    watch(() => opts.pasteTarget!.value, () => { detach(); attach() })
  }
  if (opts.dropTarget) {
    watch(() => opts.dropTarget!.value, () => { detach(); attach() })
  }

  onBeforeUnmount(detach)

  return {
    // 状态
    images,
    lastError,
    isDragging,
    canAddMore,
    remainingSlots,
    // 操作
    addFile,
    addFiles,
    removeImage,
    clearImages,
    clearError,
    toImageBlocks,
    // 事件 handlers(允许调用方手动绑定)
    onPaste,
    onDragEnter,
    onDragOver,
    onDragLeave,
    onDrop,
    // 自动绑定(需调用方在 onMounted 调用)
    attach,
    detach,
  }
}

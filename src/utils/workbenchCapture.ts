const MAX_OUTPUT_DIMENSION = 14_000
const MAX_OUTPUT_PIXELS = 50_000_000

export interface PanoramaLayout {
  width: number
  height: number
  pixelRatio: number
}

export function calculateNativeCaptureOffsets(
  viewportWidth: number,
  contentWidth: number,
): number[] {
  if (viewportWidth <= 0 || contentWidth <= 0) throw new Error('Invalid capture dimensions')
  const maxScroll = Math.max(0, Math.ceil(contentWidth - viewportWidth))
  const offsets = [0]
  for (let offset = Math.ceil(viewportWidth); offset < maxScroll; offset += Math.ceil(viewportWidth)) {
    offsets.push(offset)
  }
  if (maxScroll > 0 && offsets[offsets.length - 1] !== maxScroll) offsets.push(maxScroll)
  return offsets
}

export function calculatePanoramaLayout(
  shellWidth: number,
  shellHeight: number,
  viewportWidth: number,
  contentWidth: number,
): PanoramaLayout {
  const width = Math.ceil(shellWidth + Math.max(0, contentWidth - viewportWidth))
  const height = Math.ceil(shellHeight)
  if (width <= 0 || height <= 0) throw new Error('Invalid capture dimensions')

  const dimensionRatio = Math.min(MAX_OUTPUT_DIMENSION / width, MAX_OUTPUT_DIMENSION / height)
  const memoryRatio = Math.sqrt(MAX_OUTPUT_PIXELS / (width * height))
  const pixelRatio = Math.min(2, dimensionRatio, memoryRatio)
  if (pixelRatio < 0.5) throw new Error('Capture dimensions are too large')

  return { width, height, pixelRatio }
}

export type WorkbenchCaptureMode = 'native' | 'canvas'

export function workbenchCaptureFilename(
  tabName: string,
  mode?: WorkbenchCaptureMode,
  now = new Date(),
): string {
  const safeName = tabName
    .trim()
    .replace(/[\\/:*?"<>|]/g, '-')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 48) || 'workbench'
  const stamp = [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, '0'),
    String(now.getDate()).padStart(2, '0'),
    '-',
    String(now.getHours()).padStart(2, '0'),
    String(now.getMinutes()).padStart(2, '0'),
    String(now.getSeconds()).padStart(2, '0'),
  ].join('')
  const modeSuffix = mode ? `-${mode === 'native' ? 'webkit' : 'canvas'}` : ''
  return `monet-${safeName}${modeSuffix}-${stamp}.png`
}

import { useHtmlVisual } from './html-visual/useHtmlVisual'
import { resolveSessionCapabilities, sessionCapabilityFingerprint, type SessionCapabilityId } from './sessionCapabilities'

export function collectSessionCapabilities(): SessionCapabilityId[] {
  const { enabled: htmlVisualEnabled } = useHtmlVisual()
  return resolveSessionCapabilities({
    artifactPreview: true,
    htmlVisual: htmlVisualEnabled.value,
  })
}

export function collectSessionCapabilityFingerprint(): string {
  return sessionCapabilityFingerprint(collectSessionCapabilities())
}

import { useHtmlVisual } from './html-visual/useHtmlVisual'
import { resolveSessionCapabilities, sessionCapabilityFingerprint, type SessionCapabilityId } from './sessionCapabilities'

export function collectSessionCapabilities(): SessionCapabilityId[] {
  const { enabled: htmlVisualEnabled } = useHtmlVisual()
  return resolveSessionCapabilities({ htmlVisual: htmlVisualEnabled.value })
}

export function collectSessionCapabilityFingerprint(): string {
  return sessionCapabilityFingerprint(collectSessionCapabilities())
}

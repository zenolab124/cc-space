import { useHtmlVisual } from './html-visual/useHtmlVisual'
import { resolveSessionCapabilities, type SessionCapabilityId } from './sessionCapabilities'

export function collectSessionCapabilities(): SessionCapabilityId[] {
  const { enabled: htmlVisualEnabled } = useHtmlVisual()
  return resolveSessionCapabilities({ htmlVisual: htmlVisualEnabled.value })
}

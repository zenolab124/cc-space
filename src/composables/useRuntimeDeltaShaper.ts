import { onUnmounted, ref } from 'vue'
import { createRuntimeDeltaShaper } from '@/engines/runtimeDeltaShaper'
import type { RuntimeEventEnvelope } from '@/engines/types'

export function useRuntimeDeltaShaper(deliver: (envelope: RuntimeEventEnvelope) => void) {
  const pending = ref(false)
  const pendingTurnIds = ref<Set<string>>(new Set())
  const shaper = createRuntimeDeltaShaper({
    deliver,
    onStateChange(snapshot) {
      pending.value = snapshot.pending
      pendingTurnIds.value = snapshot.turnIds
    },
  })

  onUnmounted(shaper.reset)

  return {
    pending,
    pendingTurnIds,
    push: shaper.push,
    reset: shaper.reset,
  }
}

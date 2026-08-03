import { computed, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { engineHealth, listEngines } from './client'
import { instanceKey } from './identity'
import type { EngineDescriptor, EngineHealth } from './types'

const engines = ref<EngineDescriptor[]>([])
const health = ref<Record<string, EngineHealth>>({})
const loading = ref(false)
const errors = ref<Record<string, string>>({})
let initialized = false
let eventsInitialized = false

async function refreshEngines() {
  loading.value = true
  try {
    engines.value = await listEngines()
    const settled = await Promise.allSettled(engines.value.map(item => engineHealth(item.instance)))
    const nextHealth: Record<string, EngineHealth> = {}
    const nextErrors: Record<string, string> = {}
    settled.forEach((result, index) => {
      const key = instanceKey(engines.value[index].instance)
      if (result.status === 'fulfilled') nextHealth[key] = result.value
      else nextErrors[key] = String(result.reason)
    })
    health.value = nextHealth
    errors.value = nextErrors
  } finally {
    loading.value = false
  }
}

export function useEngines() {
  if (!initialized) {
    initialized = true
    void refreshEngines()
  }
  if (!eventsInitialized) {
    eventsInitialized = true
    void listen('engine-source-change', () => {
      // 数据视图自己处理增量；这里仅保持探活信息最终一致。
    })
  }
  return {
    engines,
    health,
    loading,
    errors,
    availableEngines: computed(() => engines.value.filter(item => item.enabled && health.value[instanceKey(item.instance)]?.source.available !== false)),
    refreshEngines,
  }
}

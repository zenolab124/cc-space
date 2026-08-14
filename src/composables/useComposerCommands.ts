import { ref, shallowRef, watch, type ComputedRef, type Ref } from 'vue'
import { listAssets } from '@/engines/client'
import type { EngineInstanceId, FacetItem } from '@/engines/types'
import type { WorkshopCommand, WorkshopSkill } from '@/types'

type ReactiveValue<T> = Ref<T> | ComputedRef<T>

function recordOf(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null
}

function stringOr(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback
}

function nullableString(value: unknown): string | null {
  return typeof value === 'string' && value ? value : null
}

function normalizeSkill(item: FacetItem): WorkshopSkill | null {
  const data = recordOf(item.data)
  if (!data) return null
  const name = stringOr(data.name, item.displayName)
  const path = stringOr(data.path)
  if (!name || !path) return null
  return {
    name,
    description: stringOr(data.description, item.description ?? ''),
    argumentHint: nullableString(data.argumentHint),
    version: nullableString(data.version),
    source: stringOr(data.source),
    scope: stringOr(data.scope),
    path,
  }
}

function normalizeCommand(item: FacetItem): WorkshopCommand | null {
  const data = recordOf(item.data)
  if (!data) return null
  const name = stringOr(data.name, item.displayName)
  const path = stringOr(data.path)
  if (!name || !path) return null
  return {
    name,
    description: stringOr(data.description, item.description ?? ''),
    argumentHint: nullableString(data.argumentHint),
    source: stringOr(data.source),
    scope: stringOr(data.scope),
    path,
  }
}

/** 按引擎 + 当前 cwd 加载输入框资产；不复用工坊的全项目聚合缓存。 */
export function useComposerCommands(
  instance: ReactiveValue<EngineInstanceId>,
  cwd: ReactiveValue<string | null | undefined>,
) {
  const skills = shallowRef<WorkshopSkill[]>([])
  const commands = shallowRef<WorkshopCommand[]>([])
  const loading = ref(false)
  const ready = ref(false)
  const error = ref<string | null>(null)
  let generation = 0

  async function load() {
    const currentCwd = cwd.value?.trim()
    const currentInstance = instance.value
    const currentGeneration = ++generation
    if (!currentCwd) {
      skills.value = []
      commands.value = []
      error.value = null
      ready.value = true
      return
    }
    loading.value = true
    ready.value = false
    error.value = null
    try {
      const [skillItems, commandItems] = await Promise.all([
        listAssets(currentInstance, 'skill', currentCwd),
        listAssets(currentInstance, 'command', currentCwd),
      ])
      if (currentGeneration !== generation) return
      skills.value = skillItems.map(normalizeSkill).filter((item): item is WorkshopSkill => !!item)
      commands.value = commandItems.map(normalizeCommand).filter((item): item is WorkshopCommand => !!item)
    } catch (cause) {
      if (currentGeneration !== generation) return
      skills.value = []
      commands.value = []
      error.value = String(cause)
    } finally {
      if (currentGeneration === generation) {
        loading.value = false
        ready.value = true
      }
    }
  }

  watch(
    [() => `${instance.value.engineId}:${instance.value.instanceId}`, () => cwd.value],
    () => { void load() },
    { immediate: true },
  )

  return { skills, commands, loading, ready, error, refresh: load }
}

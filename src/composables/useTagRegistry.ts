import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export const TAG_COLORS = [
  'sage',
  'clay',
  'ocean',
  'lavender',
  'coral',
  'sand',
  'slate',
  'ember',
] as const

export type TagColor = typeof TAG_COLORS[number]

export interface TagDefinition {
  name: string
  color: TagColor
  createdAt: string
  usageCount: number
  totalUsageCount: number
}

const tags = ref<TagDefinition[]>([])
const loading = ref(false)
const error = ref('')
const managerOpen = ref(false)
let loaded = false
let refreshRequested = false

async function loadTags(force = false) {
  if (loading.value) {
    refreshRequested ||= force
    return
  }
  if (loaded && !force) return
  loading.value = true
  error.value = ''
  try {
    tags.value = await invoke<TagDefinition[]>('get_tag_registry')
    loaded = true
  } catch (cause) {
    error.value = String(cause)
    console.warn('[tags] 标签注册表加载失败:', cause)
  } finally {
    loading.value = false
    if (refreshRequested) {
      refreshRequested = false
      void loadTags(true)
    }
  }
}

function setTags(next: TagDefinition[]) {
  tags.value = next
  loaded = true
}

export function tagColorStyle(name: string) {
  const color = tags.value.find(tag => tag.name === name)?.color ?? 'slate'
  return {
    color: `var(--tag-${color})`,
    backgroundColor: `var(--tag-${color}-wash)`,
    borderColor: `color-mix(in srgb, var(--tag-${color}) 22%, transparent)`,
  }
}

export function useTagRegistry() {
  void loadTags()

  async function renameTag(source: string, target: string) {
    setTags(await invoke<TagDefinition[]>('rename_tag', { source, target }))
  }

  async function deleteTag(name: string) {
    setTags(await invoke<TagDefinition[]>('delete_tag', { name }))
  }

  async function setTagColor(name: string, color: TagColor) {
    setTags(await invoke<TagDefinition[]>('set_tag_color', { name, color }))
  }

  return {
    tags,
    loading,
    error,
    managerOpen,
    loadTags,
    renameTag,
    deleteTag,
    setTagColor,
    openManager: () => {
      managerOpen.value = true
      void loadTags(true)
    },
    closeManager: () => { managerOpen.value = false },
  }
}

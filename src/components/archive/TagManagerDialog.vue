<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { TAG_COLORS, type TagColor, useTagRegistry } from '@/composables/useTagRegistry'
import { useSessionMeta } from '@/composables/useSessionMeta'
import { useConfirm } from '@/composables/useConfirm'
import { useSessions } from '@/composables/useSessions'
import TagChip from './TagChip.vue'

const { t } = useI18n()
const { tags, managerOpen, closeManager, renameTag, deleteTag, setTagColor } = useTagRegistry()
const { reloadMeta } = useSessionMeta()
const { confirm } = useConfirm()
const { replaceTagFilter, removeTagFilter } = useSessions()
const editingName = ref<string | null>(null)
const nameDraft = ref('')
const busyName = ref<string | null>(null)
const error = ref('')
const dialogRef = ref<HTMLElement | null>(null)
const empty = computed(() => tags.value.length === 0)

watch(managerOpen, (open) => {
  if (open) void nextTick(() => dialogRef.value?.focus())
})

function beginRename(name: string) {
  editingName.value = name
  nameDraft.value = name
  error.value = ''
}

async function commitRename(source: string) {
  const target = nameDraft.value.trim()
  if (!target || target === source) {
    editingName.value = null
    return
  }
  const collision = tags.value.some(tag => tag.name === target)
  if (collision) {
    const affected = tags.value.find(tag => tag.name === source)?.totalUsageCount ?? 0
    const approved = await confirm(
      t('archive.mergeTagConfirm', { source, target, count: affected }),
      t('archive.mergeTags'),
    )
    if (!approved) return
  }
  busyName.value = source
  error.value = ''
  try {
    await renameTag(source, target)
    await reloadMeta()
    replaceTagFilter(source, target)
    editingName.value = null
  } catch (cause) {
    error.value = String(cause)
  } finally {
    busyName.value = null
  }
}

async function remove(name: string, usageCount: number) {
  const approved = await confirm(
    t('archive.deleteTagConfirm', { tag: name, count: usageCount }),
    t('common.delete'),
  )
  if (!approved) return
  busyName.value = name
  error.value = ''
  try {
    await deleteTag(name)
    await reloadMeta()
    removeTagFilter(name)
  } catch (cause) {
    error.value = String(cause)
  } finally {
    busyName.value = null
  }
}

async function changeColor(name: string, color: TagColor) {
  busyName.value = name
  error.value = ''
  try {
    await setTagColor(name, color)
  } catch (cause) {
    error.value = String(cause)
  } finally {
    busyName.value = null
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="managerOpen"
      class="fixed inset-0 z-60 grid place-items-center bg-foreground/12 p-6"
      @mousedown.self="closeManager"
      @keydown.esc="closeManager"
    >
      <section ref="dialogRef" tabindex="-1" class="flex max-h-[min(640px,80vh)] w-[min(620px,90vw)] flex-col overflow-hidden rounded-md border border-border bg-popover shadow-paper-lifted outline-none" role="dialog" aria-modal="true" :aria-label="t('archive.manageTags')">
        <header class="flex items-center gap-3 border-b border-border px-4 py-3">
          <div class="min-w-0 flex-1">
            <h2 class="text-sm font-semibold text-foreground">{{ t('archive.manageTags') }}</h2>
            <p class="mt-0.5 text-[11px] text-muted-foreground">{{ t('archive.manageTagsHint') }}</p>
          </div>
          <button type="button" class="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" :aria-label="t('common.close')" @click="closeManager">
            <span class="i-carbon-close h-4 w-4" />
          </button>
        </header>

        <div class="min-h-0 flex-1 overflow-y-auto p-3">
          <p v-if="empty" class="py-8 text-center text-xs text-muted-foreground">{{ t('archive.noTags') }}</p>
          <template v-else>
            <div v-for="tag in tags" :key="tag.name" class="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-3 border-b border-border/50 px-1 py-2.5 last:border-b-0">
              <div class="min-w-0">
                <form v-if="editingName === tag.name" class="flex items-center gap-1.5" @submit.prevent="commitRename(tag.name)">
                  <input v-model="nameDraft" autofocus class="min-w-0 flex-1 rounded border border-input bg-background px-2 py-1 text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring" maxlength="24" />
                  <button type="submit" class="rounded bg-primary px-2 py-1 text-[10px] text-primary-foreground">{{ t('common.save') }}</button>
                  <button type="button" class="rounded border border-border px-2 py-1 text-[10px] hover:bg-muted" @click="editingName = null">{{ t('common.cancel') }}</button>
                </form>
                <div v-else class="flex min-w-0 items-center gap-2">
                  <TagChip :name="tag.name" />
                  <span class="text-[10px] text-muted-foreground">{{ t('archive.tagUsage', { count: tag.usageCount }) }}</span>
                </div>
              </div>
              <div class="flex items-center gap-1" :aria-label="t('archive.tagColor')">
                <button
                  v-for="color in TAG_COLORS"
                  :key="color"
                  type="button"
                  class="h-4 w-4 rounded-full border transition-transform hover:scale-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  :style="{
                    backgroundColor: `var(--tag-${color})`,
                    borderColor: tag.color === color ? 'var(--foreground)' : 'transparent',
                  }"
                  :aria-label="t('archive.setTagColor', { color })"
                  :aria-pressed="tag.color === color"
                  :disabled="busyName === tag.name"
                  @click="changeColor(tag.name, color)"
                />
              </div>
              <div class="flex items-center gap-1">
                <button type="button" class="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground" :title="t('common.edit')" @click="beginRename(tag.name)">
                  <span class="i-carbon-edit h-3.5 w-3.5" />
                </button>
                <button type="button" class="rounded p-1 text-muted-foreground hover:bg-destructive/10 hover:text-destructive" :title="t('common.delete')" :disabled="busyName === tag.name" @click="remove(tag.name, tag.totalUsageCount)">
                  <span class="i-carbon-trash-can h-3.5 w-3.5" />
                </button>
              </div>
            </div>
          </template>
          <p v-if="error" class="mt-2 text-[11px] text-destructive">{{ error }}</p>
        </div>
      </section>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { EngineAccent } from '@/engines/presentation'
import type { SessionSummary } from '@/types'
import { useSessionMeta } from '@/composables/useSessionMeta'
import { useTagRegistry } from '@/composables/useTagRegistry'
import TagChip from './TagChip.vue'

const props = withDefaults(defineProps<{
  session: SessionSummary
  engineName: string
  title: string
  accent?: EngineAccent
}>(), { accent: 'primary' })

const { t } = useI18n()
const { getMeta, updateMeta } = useSessionMeta()
const { tags: registryTags, openManager } = useTagRegistry()
const editing = ref(false)
const saving = ref(false)
const error = ref('')
const titleDraft = ref('')
const tagDraft = ref('')
const draftTags = ref<string[]>([])
const baselineTags = ref<string[]>([])
const accentColor = computed(() => `var(--${props.accent})`)
const metadata = computed(() => getMeta(props.session.id))
const tags = computed(() => metadata.value?.tags ?? [])
const starred = computed(() => !!metadata.value?.starred)
const suggestions = computed(() => registryTags.value.filter(tag => !draftTags.value.includes(tag.name)))
const inputListId = computed(() => `archive-tags-${props.session.id.replace(/[^a-zA-Z0-9_-]/g, '-')}`)

watch(() => props.session.id, () => {
  editing.value = false
  error.value = ''
})

watch(tags, (nextTags) => {
  if (!editing.value) return
  const baseline = new Set(baselineTags.value)
  const pendingAdditions = draftTags.value.filter(tag => !baseline.has(tag))
  baselineTags.value = [...nextTags]
  draftTags.value = [...new Set([...nextTags, ...pendingAdditions])]
})

function beginEdit() {
  titleDraft.value = metadata.value?.title || props.title
  draftTags.value = [...tags.value]
  baselineTags.value = [...tags.value]
  tagDraft.value = ''
  error.value = ''
  editing.value = true
}

function toggleEdit() {
  if (editing.value) editing.value = false
  else beginEdit()
}

function addDraftTags() {
  const additions = tagDraft.value
    .split(/[，,、;；]/)
    .map(tag => tag.trim())
    .filter(Boolean)
  draftTags.value = [...new Set([...draftTags.value, ...additions])]
  tagDraft.value = ''
}

async function save() {
  addDraftTags()
  saving.value = true
  error.value = ''
  try {
    await updateMeta(props.session.id, {
      title: titleDraft.value.trim(),
      tags: draftTags.value,
    }, props.session.reference)
    editing.value = false
  } catch (cause) {
    error.value = String(cause)
  } finally {
    saving.value = false
  }
}

async function toggleStar() {
  try {
    await updateMeta(props.session.id, { starred: !starred.value }, props.session.reference)
  } catch (cause) {
    error.value = String(cause)
  }
}
</script>

<template>
  <div class="shrink-0 border-b border-border bg-card">
    <header class="flex min-h-9 items-center gap-2 px-3 py-1.5">
      <span
        class="shrink-0 rounded border px-1.5 py-0.5 text-[10px] font-semibold"
        :style="{
          color: accentColor,
          borderColor: `color-mix(in srgb, ${accentColor} 22%, transparent)`,
          background: `color-mix(in srgb, ${accentColor} 10%, transparent)`,
        }"
      >{{ engineName }}</span>
      <div class="min-w-0 flex-1 truncate text-xs font-semibold">{{ title }}</div>
      <div v-if="tags.length" class="hidden min-w-0 items-center gap-1 xl:flex">
        <TagChip v-for="tag in tags.slice(0, 3)" :key="tag" :name="tag" compact />
        <span v-if="tags.length > 3" class="text-[10px] text-muted-foreground">+{{ tags.length - 3 }}</span>
      </div>
      <button
        type="button"
        class="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        :style="starred ? { color: 'var(--star)' } : undefined"
        :title="starred ? t('archive.unstar') : t('archive.star')"
        :aria-pressed="starred"
        @click="toggleStar"
      >
        <span class="h-3.5 w-3.5" :class="starred ? 'i-carbon-star-filled' : 'i-carbon-star'" />
      </button>
      <button
        type="button"
        class="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        :title="t('archive.editMetadata')"
        :aria-expanded="editing"
        @click="toggleEdit"
      >
        <span class="i-carbon-edit h-3.5 w-3.5" />
      </button>
    </header>
    <p v-if="error && !editing" class="border-t border-border/70 px-3 py-1 text-[10px] text-destructive">{{ error }}</p>

    <form v-if="editing" class="flex flex-wrap items-end gap-2 border-t border-border/70 px-3 py-2" @submit.prevent="save">
      <label class="min-w-40 flex-1 text-[10px] text-muted-foreground">
        <span class="mb-1 block">{{ t('archive.sessionTitle') }}</span>
        <input v-model="titleDraft" class="w-full rounded border border-input bg-background px-2 py-1.5 text-xs text-foreground outline-none focus:border-ring focus:ring-1 focus:ring-ring" />
      </label>
      <div class="min-w-56 flex-[1.25]">
        <div class="mb-1 flex items-center justify-between text-[10px] text-muted-foreground">
          <span>{{ t('archive.tags') }}</span>
          <button type="button" class="hover:text-foreground" @click="openManager">{{ t('archive.manageTags') }}</button>
        </div>
        <div class="flex min-h-8 flex-wrap items-center gap-1 rounded border border-input bg-background px-1.5 py-1 focus-within:border-ring focus-within:ring-1 focus-within:ring-ring">
          <TagChip
            v-for="tag in draftTags"
            :key="tag"
            :name="tag"
            compact
            removable
            @remove="draftTags = draftTags.filter(value => value !== tag)"
          />
          <input
            v-model="tagDraft"
            class="min-w-20 flex-1 bg-transparent px-1 py-0.5 text-xs text-foreground outline-none"
            :list="inputListId"
            :placeholder="draftTags.length ? '' : t('archive.tagInputPlaceholder')"
            @keydown.enter.prevent="addDraftTags"
            @blur="addDraftTags"
          />
          <datalist :id="inputListId">
            <option v-for="tag in suggestions" :key="tag.name" :value="tag.name" />
          </datalist>
        </div>
      </div>
      <div class="flex gap-1.5">
        <button type="button" class="rounded border border-border px-2.5 py-1.5 text-xs hover:bg-muted" @click="editing = false">{{ t('common.cancel') }}</button>
        <button type="submit" class="rounded bg-primary px-2.5 py-1.5 text-xs text-primary-foreground disabled:opacity-50" :disabled="saving">{{ t('common.save') }}</button>
      </div>
      <p v-if="error" class="w-full text-[10px] text-destructive">{{ error }}</p>
    </form>
  </div>
</template>

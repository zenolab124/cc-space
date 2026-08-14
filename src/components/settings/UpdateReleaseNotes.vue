<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { openExternalUrl } from '@/composables/useFileOpener'
import {
  parseReleaseNotes,
  releaseNotesUrl,
  type ReleaseChannel,
  type ReleaseNoteType,
} from '@/utils/releaseNotes'

const props = defineProps<{
  notes: string
  version: string
  locale: string
  channel: ReleaseChannel
}>()

const { t } = useI18n()
const expanded = ref(false)
const parsed = computed(() => parseReleaseNotes(props.notes, props.locale, props.version))
const visibleItems = computed(() => {
  const items = parsed.value?.content.items ?? []
  return expanded.value ? items : items.slice(0, 3)
})
const hiddenCount = computed(() => Math.max(0, (parsed.value?.content.items.length ?? 0) - 3))

const typeMeta: Record<ReleaseNoteType, { icon: string; label: string }> = {
  new: { icon: 'i-carbon-add-alt', label: 'settings.updateNotesNew' },
  improved: { icon: 'i-carbon-improve-relevance', label: 'settings.updateNotesImproved' },
  fixed: { icon: 'i-carbon-checkmark-outline', label: 'settings.updateNotesFixed' },
}

watch(() => [props.notes, props.version], () => {
  expanded.value = false
})

function openFullNotes() {
  void openExternalUrl(releaseNotesUrl(props.version, props.channel))
}
</script>

<template>
  <section v-if="parsed" class="update-notes" :aria-label="t('settings.updateNotesTitle')">
    <div class="update-notes-heading">
      <div class="min-w-0">
        <div class="update-notes-kicker">{{ t('settings.updateNotesTitle') }}</div>
        <p class="update-notes-summary">{{ parsed.content.summary }}</p>
      </div>
      <span class="update-version-badge">v{{ version }}</span>
    </div>

    <ul v-if="parsed.content.items.length" class="update-note-list">
      <li v-for="(item, index) in visibleItems" :key="`${item.type}-${index}-${item.title}`" class="update-note-item">
        <span class="update-note-icon" :data-type="item.type" aria-hidden="true">
          <span :class="typeMeta[item.type].icon" />
        </span>
        <div class="min-w-0">
          <div class="update-note-title-row">
            <span class="update-note-type">{{ t(typeMeta[item.type].label) }}</span>
            <span class="update-note-title">{{ item.title }}</span>
          </div>
          <p v-if="item.detail" class="update-note-detail">{{ item.detail }}</p>
        </div>
      </li>
    </ul>

    <div class="update-notes-actions">
      <button
        v-if="hiddenCount > 0 || expanded"
        type="button"
        class="update-note-link"
        :aria-expanded="expanded"
        @click="expanded = !expanded"
      >
        {{ expanded ? t('settings.updateNotesCollapse') : t('settings.updateNotesMore', { count: hiddenCount }) }}
        <span :class="expanded ? 'i-carbon-chevron-up' : 'i-carbon-chevron-down'" aria-hidden="true" />
      </button>
      <button type="button" class="update-note-link ml-auto" @click="openFullNotes">
        {{ t('settings.updateNotesFull') }}
        <span class="i-carbon-launch" aria-hidden="true" />
      </button>
    </div>
  </section>
</template>

<style scoped>
.update-notes {
  margin-top: 12px;
  padding: 12px;
  border: 1px solid color-mix(in srgb, var(--primary) 24%, var(--border));
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--primary) 4%, var(--card));
}
.update-notes-heading {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}
.update-notes-kicker {
  margin-bottom: 3px;
  color: var(--primary);
  font-size: 9.5px;
  font-weight: 650;
  letter-spacing: 0.08em;
}
.update-notes-summary {
  margin: 0;
  color: var(--foreground);
  font-size: 12.5px;
  font-weight: 600;
  line-height: 1.55;
}
.update-version-badge {
  flex-shrink: 0;
  padding: 2px 6px;
  border: 1px solid color-mix(in srgb, var(--primary) 45%, var(--border));
  border-radius: calc(var(--radius) - 2px);
  color: var(--primary);
  background: var(--card);
  font-family: var(--font-mono);
  font-size: 9.5px;
  line-height: 1.4;
}
.update-note-list {
  display: grid;
  gap: 6px;
  margin: 10px 0 0;
  padding: 0;
  list-style: none;
}
.update-note-item {
  display: grid;
  grid-template-columns: 24px minmax(0, 1fr);
  gap: 8px;
  padding: 7px 8px;
  border: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
  border-radius: var(--radius);
  background: var(--card);
}
.update-note-icon {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: calc(var(--radius) - 2px);
  color: var(--primary);
  background: color-mix(in srgb, var(--primary) 10%, transparent);
  font-size: 13px;
}
.update-note-icon[data-type='fixed'] {
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 10%, transparent);
}
.update-note-title-row {
  display: flex;
  align-items: baseline;
  gap: 6px;
  min-height: 18px;
}
.update-note-type {
  flex-shrink: 0;
  color: var(--muted-foreground);
  font-size: 9.5px;
  font-weight: 600;
}
.update-note-title {
  color: var(--foreground);
  font-size: 11.5px;
  font-weight: 550;
  line-height: 1.5;
}
.update-note-detail {
  margin: 2px 0 0;
  color: var(--muted-foreground);
  font-size: 10.5px;
  line-height: 1.6;
}
.update-notes-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 9px;
}
.update-note-link {
  display: inline-flex;
  align-items: center;
  min-height: 28px;
  gap: 4px;
  padding: 2px 4px;
  border-radius: calc(var(--radius) - 2px);
  color: var(--muted-foreground);
  font-size: 10.5px;
  transition: color 150ms ease-out, background-color 150ms ease-out;
}
.update-note-link:hover {
  color: var(--foreground);
  background: var(--muted);
}
.update-note-link:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
}
@media (max-width: 620px) {
  .update-notes-heading { gap: 8px; }
  .update-note-title-row { align-items: flex-start; flex-direction: column; gap: 0; }
}
@media (prefers-reduced-motion: reduce) {
  .update-note-link { transition: none; }
}
</style>

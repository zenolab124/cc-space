<script setup lang="ts">
import { computed } from 'vue'
import { tagColorStyle } from '@/composables/useTagRegistry'

const props = withDefaults(defineProps<{
  name: string
  active?: boolean
  clickable?: boolean
  removable?: boolean
  compact?: boolean
}>(), {
  active: false,
  clickable: false,
  removable: false,
  compact: false,
})

const emit = defineEmits<{
  click: [event: Event]
  remove: []
}>()

const style = computed(() => ({
  ...tagColorStyle(props.name),
  boxShadow: props.active ? '0 0 0 1px currentColor inset' : undefined,
}))

function activate(event: Event) {
  if (props.clickable) emit('click', event)
}
</script>

<template>
  <span
    class="inline-flex min-w-0 items-center rounded border font-medium leading-none"
    :class="[
      compact ? 'gap-0.5 px-1.5 py-0.5 text-[9px]' : 'gap-1 px-2 py-1 text-[10px]',
      clickable ? 'cursor-pointer transition-[filter,box-shadow] hover:brightness-95 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring' : '',
    ]"
    :style="style"
    :role="clickable ? 'button' : undefined"
    :tabindex="clickable ? 0 : undefined"
    :aria-pressed="clickable ? active : undefined"
    :title="name"
    @click="activate"
    @keydown.enter.prevent="activate"
    @keydown.space.prevent="activate"
  >
    <span class="truncate">{{ name }}</span>
    <button
      v-if="removable"
      type="button"
      class="-mr-0.5 inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded hover:bg-foreground/10 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      :aria-label="$t('archive.removeTag', { tag: name })"
      @click.stop="emit('remove')"
    >
      <span class="i-carbon-close h-2.5 w-2.5" />
    </button>
  </span>
</template>

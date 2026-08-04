<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue'

const props = withDefaults(defineProps<{
  modelValue: string
  placeholder?: string
  disabled?: boolean
  maxHeight?: number
}>(), {
  placeholder: '',
  disabled: false,
  maxHeight: 160,
})

const emit = defineEmits<{
  (event: 'update:modelValue', value: string): void
  (event: 'keydown', value: KeyboardEvent): void
  (event: 'input', value: Event): void
  (event: 'keyup', value: KeyboardEvent): void
  (event: 'click', value: MouseEvent): void
  (event: 'select', value: Event): void
}>()

const element = ref<HTMLTextAreaElement>()

function resize() {
  const textarea = element.value
  if (!textarea) return
  textarea.style.height = 'auto'
  textarea.style.height = `${Math.min(textarea.scrollHeight, props.maxHeight)}px`
}

function resetHeight() {
  if (element.value) element.value.style.height = 'auto'
}

function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLTextAreaElement).value)
  resize()
  emit('input', event)
}

watch(() => props.modelValue, () => nextTick(resize))
onMounted(resize)

defineExpose({ element, resize, resetHeight })
</script>

<template>
  <textarea
    ref="element"
    :value="modelValue"
    :placeholder="placeholder"
    :disabled="disabled"
    rows="1"
    @keydown="emit('keydown', $event)"
    @input="onInput"
    @keyup="emit('keyup', $event)"
    @click="emit('click', $event)"
    @select="emit('select', $event)"
  />
</template>

import { computed, ref } from 'vue'
import { foldDefaultExpanded, foldDefaultRevision, setFoldDefault } from './useFoldDefaults'

/**
 * 普通点击只控制当前思考块；Shift + 点击修改并持久化全局默认，
 * 同步当前所有思考块，后续新渲染与下次启动也跟随该默认。
 */

export function useThinkingExpand() {
  const localExpanded = ref<boolean | null>(null)
  const localRevision = ref(foldDefaultRevision.thinking.value)
  const thinkingExpanded = computed({
    get: () => localExpanded.value !== null
      && localRevision.value === foldDefaultRevision.thinking.value
      ? localExpanded.value
      : foldDefaultExpanded.thinking.value,
    set: (value: boolean) => {
      localExpanded.value = value
      localRevision.value = foldDefaultRevision.thinking.value
    },
  })

  function toggle(event?: Pick<MouseEvent, 'shiftKey'>) {
    const next = !thinkingExpanded.value
    if (event?.shiftKey) {
      setFoldDefault('thinking', next)
      return
    }
    thinkingExpanded.value = next
  }

  return { thinkingExpanded, toggle }
}

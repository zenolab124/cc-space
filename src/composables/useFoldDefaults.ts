import { ref, type Ref } from 'vue'
import { readMigratedStorage } from '@/utils/storageMigrate'

export type FoldDefaultKind = 'thinking' | 'toolGroup' | 'toolItem'

const STORAGE_KEYS: Record<FoldDefaultKind, string> = {
  thinking: 'monet:thinking-expanded',
  toolGroup: 'monet:tool-group-expanded',
  toolItem: 'monet:tool-item-expanded',
}

function load(kind: FoldDefaultKind): boolean {
  try {
    if (kind === 'thinking') {
      return readMigratedStorage(STORAGE_KEYS[kind], 'cc-space:thinking-expanded') === '1'
    }
    return localStorage.getItem(STORAGE_KEYS[kind]) === '1'
  } catch {
    return false
  }
}

/** 模块级默认值让所有常驻视图即时同步；revision 保证重复设置同值也能清掉逐项覆盖。 */
export const foldDefaultExpanded: Record<FoldDefaultKind, Ref<boolean>> = {
  thinking: ref(load('thinking')),
  toolGroup: ref(load('toolGroup')),
  toolItem: ref(load('toolItem')),
}

export const foldDefaultRevision: Record<FoldDefaultKind, Ref<number>> = {
  thinking: ref(0),
  toolGroup: ref(0),
  toolItem: ref(0),
}

export function setFoldDefault(kind: FoldDefaultKind, expanded: boolean): void {
  foldDefaultExpanded[kind].value = expanded
  foldDefaultRevision[kind].value += 1
  try {
    localStorage.setItem(STORAGE_KEYS[kind], expanded ? '1' : '0')
  } catch {
    // 存储失败只影响跨启动记忆，不阻断当前页面同步。
  }
}

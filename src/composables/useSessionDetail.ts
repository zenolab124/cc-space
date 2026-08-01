import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { SessionRecord } from '@/types'
import { probeSessionLoad } from '@/utils/perfProbe'

/** 每次调用创建独立实例，支持工作台多列场景 */
export function createSessionDetail() {
  const records = ref<SessionRecord[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const currentProjectId = ref<string | null>(null)
  const currentSessionId = ref<string | null>(null)
  /** 屏外冷卸载只清 records，保留身份；重新可见时据此强制恢复。 */
  const recordsReleased = ref(false)

  watch(records, value => {
    if (value.length > 0) recordsReleased.value = false
  })

  async function loadRecords(projectId: string, sessionId: string, force = false, fallbackSessionId?: string) {
    if (!force && currentProjectId.value === projectId && currentSessionId.value === sessionId) {
      return
    }

    loading.value = true
    error.value = null
    currentProjectId.value = projectId
    currentSessionId.value = sessionId

    const probe = probeSessionLoad(sessionId)
    try {
      records.value = await invoke<SessionRecord[]>('get_session_records', {
        projectId,
        sessionId,
      })
      // 分叉垫底:自有 jsonl 未落盘(CLI 首条消息才写)时以源会话历史垫底显示,
      // 落盘后 records 非空自然走自有数据
      if (!records.value.length && fallbackSessionId) {
        try {
          records.value = await invoke<SessionRecord[]>('get_session_records', {
            projectId,
            sessionId: fallbackSessionId,
          })
        } catch (_) { /* 源会话读取失败保持空态,不算错误 */ }
      }
      probe?.afterInvoke(records.value.length)
      probe?.afterAssign()
      recordsReleased.value = false
    } catch (e) {
      error.value = String(e)
      records.value = []
    } finally {
      loading.value = false
    }
  }

  /** 强制重新加载（流式结束后） */
  async function reloadRecords() {
    if (currentProjectId.value && currentSessionId.value) {
      const pid = currentProjectId.value
      const sid = currentSessionId.value
      // 清除缓存让 loadRecords 重新加载
      currentProjectId.value = null
      currentSessionId.value = null
      await loadRecords(pid, sid)
    }
  }

  function clearRecords() {
    records.value = []
    currentProjectId.value = null
    currentSessionId.value = null
    error.value = null
    recordsReleased.value = false
  }

  /**
   * 释放屏外列的历史数据与派生 DOM；流式状态由 useStreaming 单例持有，不受影响。
   * 与 clearRecords 不同，此处保留项目/会话身份，让恢复路径仍能识别当前数据源。
   */
  function releaseRecords() {
    if (records.value.length === 0) return
    records.value = []
    error.value = null
    recordsReleased.value = true
  }

  return {
    records,
    loading,
    error,
    currentProjectId,
    currentSessionId,
    recordsReleased,
    loadRecords,
    reloadRecords,
    clearRecords,
    releaseRecords,
  }
}

// 向后兼容：默认单例（非分屏模式用）
const defaultInstance = createSessionDetail()
export function useSessionDetail() {
  return defaultInstance
}

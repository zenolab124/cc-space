/**
 * localStorage key 更名迁移读取。
 *
 * 项目从 cc-space-* 更名为 monet-*,旧版本写入的数据仍存于旧 key。此工具在读取处
 * 做一次性向后兼容:优先读新 key;新 key 为空时回落旧 key,读到则顺手写入新 key。
 * 旧 key 保留不删(倾向保守,避免与仍可能运行的旧版本互相干扰)。
 *
 * @param newKey    当前(monet 前缀)key
 * @param legacyKey 旧(cc-space 前缀)key
 * @returns 新 key 值;新 key 为空则返回迁移后的旧 key 值;皆空返回 null
 */
export function readMigratedStorage(newKey: string, legacyKey: string): string | null {
  if (typeof localStorage === 'undefined') return null
  const cur = localStorage.getItem(newKey)
  if (cur !== null) return cur
  const legacy = localStorage.getItem(legacyKey)
  if (legacy !== null) {
    // 旧 key 迁移:读到旧值写入新 key(旧 key 保留)
    try { localStorage.setItem(newKey, legacy) } catch { /* 写失败不阻塞读取 */ }
  }
  return legacy
}

import { invoke } from '@tauri-apps/api/core'

/**
 * UI 偏好双源桥:localStorage 为同步镜像(保首帧零闪),
 * ~/.monet/settings.json 为权威源(供外部工具/AI 预写与修改,重启生效)。
 *
 * 启动对账:文件有值 → apply 应用到运行态;文件无值而镜像有 → 一次性上迁镜像值。
 * app 运行中外部直改文件不热加载,下次启动经此对账生效。
 */
export function bridgeSetting(opts: {
  /** settings.json 顶层键 */
  key: string
  /** 文件无值时的一次性上迁初值(通常取镜像现值);返回 undefined 跳过上迁 */
  uplift: () => unknown
  /** 文件权威值到达:校验后应用到运行态(镜像由调用方原有持久化逻辑刷新);无效值忽略 */
  apply: (value: unknown) => void
}): void {
  invoke<unknown>('get_app_setting', { key: opts.key })
    .then(v => {
      if (v === null || v === undefined) {
        const init = opts.uplift()
        if (init !== undefined) {
          invoke('set_app_setting', { key: opts.key, value: init }).catch(() => {})
        }
        return
      }
      opts.apply(v)
    })
    .catch(() => {}) // 桥接失败退回纯镜像模式,不阻塞启动
}

/** 双写:运行态变更同步进 settings.json(镜像仍由调用方原有逻辑维护) */
export function writeSetting(key: string, value: unknown) {
  invoke('set_app_setting', { key, value }).catch(() => {})
}

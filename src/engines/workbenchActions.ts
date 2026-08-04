import type { EngineCapabilities } from './types'

export interface WorkbenchEngineActions {
  create: boolean
  fork: boolean
  race: boolean
}

/** 工作台只消费标准 capability；引擎专属按钮由各自控制器另外声明。 */
export function resolveWorkbenchEngineActions(
  runtime: EngineCapabilities['runtime'] | null | undefined,
  nativeSurface: boolean,
): WorkbenchEngineActions {
  const create = nativeSurface || runtime?.create === true
  const fork = nativeSurface || runtime?.fork === true
  return { create, fork, race: create && fork }
}

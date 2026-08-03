import { reactive, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { WorkshopAssets } from '../types'
import { listAssets, listEngines } from '@/engines/client'
import type { EngineDescriptor } from '@/engines/types'

/**
 * 工坊数据源（v2.3.0 FR-001/FR-005）：
 * 首次进入工坊惰性触发 get_workshop_assets，会话期内存缓存，手动刷新强制重调。
 * MCP 探活结果不持久化、不轮询。模块级单例，与 useHomeStats 同模式。
 */

/** 工坊子页类别（同时是 open_workshop_dir 的 category 入参口径） */
export type WorkshopCategory = 'skills' | 'commands' | 'agents' | 'mcp' | 'hooks' | 'memory'

/** MCP 探活状态（stdio 不参与，恒显「stdio · 未探活」） */
export type McpProbeState = 'probing' | 'online' | 'offline'

const assets = ref<WorkshopAssets | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const sourceEngine = ref<EngineDescriptor | null>(null)

/** 探活状态表，key = path + '|' + name（同名服务器跨配置文件唯一） */
const probeStates = reactive(new Map<string, McpProbeState>())

let loadedOnce = false
/** 探测代次：新一轮探测启动后，旧轮次回调作废，避免过期结果覆盖 */
let probeGen = 0

/** MCP 探活状态表的 key */
export function mcpKey(s: { path: string; name: string }): string {
  return `${s.path}|${s.name}`
}

async function load() {
  loading.value = true
  error.value = null
  try {
    const descriptor = (await listEngines()).find(engine => engine.enabled && engine.capabilities.facets.assets)
    if (!descriptor) throw new Error('No enabled engine provides workshop assets')
    sourceEngine.value = descriptor
    const [skills, commands, agents, mcpServers] = await Promise.all([
      listAssets(descriptor.instance, 'skill'),
      listAssets(descriptor.instance, 'command'),
      listAssets(descriptor.instance, 'agent'),
      listAssets(descriptor.instance, 'mcp'),
    ])
    assets.value = {
      skills: skills.map(item => item.data) as WorkshopAssets['skills'],
      commands: commands.map(item => item.data) as WorkshopAssets['commands'],
      agents: agents.map(item => item.data) as WorkshopAssets['agents'],
      mcpServers: mcpServers.map(item => item.data) as WorkshopAssets['mcpServers'],
    }
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

/** 首次进入工坊时调用；会话期内已加载则直接用缓存 */
function ensureLoaded() {
  if (loadedOnce) return
  loadedOnce = true
  load()
}

/** 页头刷新：加载中防重入 */
async function refresh() {
  if (loading.value) return
  await load()
}

/**
 * 对当前 mcpServers 中 http/sse 服务器并发探活（FR-005）：
 * 列表先渲染，徽章从「探测中…」异步落到在线/离线；stdio 不发任何请求；
 * invoke 抛错按「离线」处理。
 */
function probeMcpServers() {
  const servers = assets.value?.mcpServers ?? []
  const gen = ++probeGen
  probeStates.clear()
  for (const s of servers) {
    if (s.transport !== 'http' && s.transport !== 'sse') continue
    const key = mcpKey(s)
    probeStates.set(key, 'probing')
    invoke<boolean>('probe_mcp_server', { url: s.endpoint })
      .then((ok) => {
        if (gen === probeGen) probeStates.set(key, ok ? 'online' : 'offline')
      })
      .catch(() => {
        if (gen === probeGen) probeStates.set(key, 'offline')
      })
  }
}

export function useWorkshop() {
  return {
    assets,
    sourceEngine,
    loading,
    error,
    ensureLoaded,
    refresh,
    retry: load,
    probeStates,
    probeMcpServers,
  }
}

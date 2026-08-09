import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('定时任务默认引擎', () => {
  it('表单只提供声明自动化能力的引擎，并按运行时健康状态禁用', () => {
    const form = source('../../src/components/automation/RoutineForm.vue')

    expect(form).toContain('engine.capabilities.facets.automation')
    expect(form).toContain('health.value[instanceKey(engine.instance)]?.runtime.available === true')
    expect(form).toContain('engine: selectedEngine.value.descriptor.instance')
    expect(form).toContain(':disabled="saving || enginesLoading || !selectedEngineAvailable"')
  })

  it('前台立即运行与后台 runner 共用命令及输出适配', () => {
    const app = source('../../src-tauri/src/routines.rs')
    const runner = source('../../src-tauri/src/bin/monet_routine_runner.rs')

    for (const implementation of [app, runner]) {
      expect(implementation).toContain('build_routine_command')
      expect(implementation).toContain('normalize_routine_stdout')
      expect(implementation).toContain('engine.is_codex()')
    }
  })

  it('Codex 声明自动化能力，MCP 允许显式选择执行引擎', () => {
    const adapter = source('../../src-tauri/src/engines/codex/adapter.rs')
    const mcp = source('../../src-tauri/src/bin/monet_mcp.rs')

    expect(adapter).toMatch(/automation:\s*true/)
    expect(mcp).toContain('"enum": ["claude-code", "codex"]')
    expect(mcp).toContain('RoutineEngine::codex()')
  })

  it('任务表格支持逐条切换与单次原子切换全部任务', () => {
    const view = source('../../src/views/AutomationView.vue')
    const composable = source('../../src/composables/useRoutines.ts')
    const backend = source('../../src-tauri/src/routines.rs')
    const bridge = source('../../src-tauri/src/lib.rs')

    expect(view).toContain('@change="onRoutineEngineChange(r, $event)"')
    expect(view).toContain('@change="onAllRoutineEnginesChange"')
    expect(view).toContain('bulkEngineSwitching')
    expect(view).toContain('role="alert"')
    expect(composable).toContain("invoke<number>('update_all_routine_engines', { engine })")
    expect(backend).toContain('replace_routine_engines(routines, &engine)')
    expect(bridge).toContain('routines::update_all_routine_engines')
  })
})

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('多引擎统计聚合', () => {
  it('共享统计层合并 Codex 并保留引擎分项', () => {
    const usage = source('../../src-tauri/src/usage_stats.rs')

    expect(usage).toContain('crate::engines::codex::collect_local_usage()')
    expect(usage).toContain('#[serde(rename = "byEngine")]')
    expect(usage).toContain('engines.entry(contribution.engine_id.clone())')
  })

  it('Widget 会话与项目统计消费聚合会话活动', () => {
    const updater = source('../../src-tauri/src/bin/widget_updater.rs')

    expect(updater).toContain('stats.sessions')
    expect(updater).toContain('collect_project_stats(start_ts, &stats.sessions)')
    expect(updater).not.toContain('collect_jsonl(&config::projects_dir()')
  })

  it('共享统计 IPC 不暴露本地会话路径', () => {
    const usage = source('../../src-tauri/src/usage_stats.rs')

    expect(usage).toMatch(/#\[serde\(skip\)\]\s+pub sessions: Vec<SessionActivity>/)
  })

  it('首页不再用 Claude-only 项目列表覆盖 Widget 快照', () => {
    const home = source('../../src/views/HomeView.vue')

    expect(home).not.toContain("invoke('update_widget'")
  })
})

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('intelligent augmentation engine parity', () => {
  it('uses SessionRef and normalized engine timelines for generated metadata', () => {
    const metadata = source('../../src-tauri/src/metadata.rs')
    const sessionMeta = source('../../src/composables/useSessionMeta.ts')

    expect(metadata).toContain('pub async fn generate_title(session: SessionRef)')
    expect(metadata).toContain('pub async fn generate_tags(session: SessionRef)')
    expect(metadata).toContain('pub async fn generate_summary(session: SessionRef)')
    expect(metadata).toContain('.load_timeline(')
    expect(metadata).toContain('metadata_for_ref(&session)')
    expect(metadata).toMatch(/store\.update\(\s*&session,/)
    expect(metadata).not.toContain('projects_dir()')
    expect(sessionMeta).toContain("invoke<{ title: string, turnCount: number }>('generate_title', { session })")
    expect(sessionMeta).toContain("invoke<{ tags: string[], skipped: boolean }>('generate_tags', { session })")
    expect(sessionMeta).toContain("shouldRefresh(sessionId, 'tagsManual')")
    expect(sessionMeta).toContain("invoke<string>('generate_summary', { session })")
  })

  it('exposes summary generation and automatic metadata refresh on the standard surface', () => {
    const controller = source('../../src/components/engine/EngineSessionDetail.vue')

    expect(controller).toContain("const { getMeta, updateMeta, refreshSummary } = useSessionMeta()")
    expect(controller).toContain('await refreshSummary(target, true)')
    expect(controller).toContain("t('archive.generateSummary')")
    expect(controller).toContain('triggerMetaGeneration(reference.value)')
  })

  it('adds intelligent hints to standard engine approval cards', () => {
    const controller = source('../../src/components/engine/EngineSessionDetail.vue')

    expect(controller).toContain('requestHint(')
    expect(controller).toContain('getHint(request.reference.requestId)?.loading')
    expect(controller).toContain("t('permission.analyzing')")
    expect(controller).toContain('clearHint(request.reference.requestId)')
  })

  it('runs smart search against engine adapters and preserves structured session identity', () => {
    const commands = source('../../src-tauri/src/commands.rs')
    const agentSearch = source('../../src/composables/useAgentSearch.ts')

    const smartSearch = commands.slice(
      commands.indexOf('pub async fn smart_search('),
      commands.indexOf('/// schema-probe'),
    )
    expect(smartSearch).toContain('crate::engines::commands::engine_search(')
    expect(smartSearch).toContain('hit.session.storage_key()')
    expect(smartSearch).not.toContain('search::query(')
    expect(agentSearch).toContain('sessionUiId(hit.session)')
    expect(agentSearch).toContain('resolveSession(sessionId)')
  })
})

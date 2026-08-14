import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { parseReleaseNotes, releaseNotesUrl } from '../../src/utils/releaseNotes'
import {
  createNightlyReleaseNotes,
  renderReleaseNotesMarkdown,
  validateReleaseNotes,
} from '../../scripts/release-notes.mjs'

const structuredNotes = {
  schema: 1,
  version: '1.2.3',
  locales: {
    'zh-CN': {
      summary: '中文摘要',
      items: [
        { type: 'new', title: '新增能力' },
        { type: 'fixed', title: '修复问题', detail: '补充说明' },
      ],
    },
    'en-US': {
      summary: 'English summary',
      items: [
        { type: 'new', title: 'New capability' },
        { type: 'fixed', title: 'Fixed an issue', detail: 'More detail' },
      ],
    },
  },
}

describe('release notes', () => {
  it('按界面语言选择内容，并让其他语言回退英文', () => {
    const raw = JSON.stringify(structuredNotes)
    expect(parseReleaseNotes(raw, 'zh-CN')?.content.summary).toBe('中文摘要')
    expect(parseReleaseNotes(raw, 'ja-JP')?.content.summary).toBe('English summary')
  })

  it('旧版纯文本安全降级，不保留 HTML 标签', () => {
    const parsed = parseReleaseNotes('## Update\n- Fixed <b>rendering</b>', 'en-US')
    expect(parsed?.structured).toBe(false)
    expect(parsed?.content.summary).toContain('Fixed rendering')
    expect(parsed?.content.summary).not.toContain('<b>')
  })

  it('拒绝版本错配和双语结构漂移', () => {
    expect(() => validateReleaseNotes(structuredNotes, '1.2.4')).toThrow('不一致')
    expect(parseReleaseNotes(JSON.stringify(structuredNotes), 'zh-CN', '1.2.4')).toBeNull()
    expect(() => validateReleaseNotes({
      ...structuredNotes,
      locales: {
        ...structuredNotes.locales,
        'en-US': { ...structuredNotes.locales['en-US'], items: structuredNotes.locales['en-US'].items.slice(1) },
      },
    }, '1.2.3')).toThrow('数量和类型顺序')
  })

  it('从同一份结构化内容生成 GitHub 用户摘要', () => {
    const markdown = renderReleaseNotesMarkdown(structuredNotes, '1.2.3')
    expect(markdown).toContain('## 本次更新')
    expect(markdown).toContain("## What's new in Monet")
    expect(markdown).toContain('- 修复问题 — 补充说明')
  })

  it('Nightly 去重并限制提交条目数量', () => {
    const notes = createNightlyReleaseNotes('1.2.4', [
      'fix: one',
      'fix: one',
      ...Array.from({ length: 12 }, (_, index) => `change ${index}`),
    ])
    expect(notes.locales['zh-CN'].items).toHaveLength(8)
    expect(notes.locales['zh-CN'].items[0].title).toBe('fix: one')
  })

  it('Nightly 将超长提交标题安全截断到校验上限', () => {
    const notes = createNightlyReleaseNotes('1.2.4', ['🚀'.repeat(100)])
    const title = notes.locales['zh-CN'].items[0].title
    expect(title.length).toBeLessThanOrEqual(180)
    expect(title.endsWith('…')).toBe(true)
    expect(title).not.toContain('�')
  })

  it('按更新通道生成正确的完整发布说明链接', () => {
    expect(releaseNotesUrl('1.2.3', 'stable')).toBe('https://github.com/zenolab124/monet/releases/tag/v1.2.3')
    expect(releaseNotesUrl('1.2.4', 'nightly')).toBe('https://github.com/zenolab124/monet/releases/tag/nightly')
  })

  it('将结构化说明写进 Tauri updater manifest', () => {
    const dir = mkdtempSync(join(tmpdir(), 'monet-release-notes-'))
    try {
      const tarball = join(dir, 'Monet_1.2.3.app.tar.gz')
      const notesFile = join(dir, 'notes.json')
      const output = join(dir, 'latest.json')
      writeFileSync(tarball, 'artifact')
      writeFileSync(`${tarball}.sig`, 'test-signature')
      writeFileSync(notesFile, JSON.stringify(structuredNotes))

      execFileSync(process.execPath, [
        new URL('../../scripts/create-latest-json.mjs', import.meta.url).pathname,
        '1.2.3',
        tarball,
        output,
      ], {
        env: { ...process.env, RELEASE_NOTES_FILE: notesFile },
      })

      const manifest = JSON.parse(readFileSync(output, 'utf8')) as { notes: string }
      expect(JSON.parse(manifest.notes)).toMatchObject({ schema: 1, version: '1.2.3' })
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

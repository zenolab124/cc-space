import { describe, expect, it } from 'vitest'
import type { WorkshopCommand, WorkshopSkill } from '../../src/types'
import {
  filterCommands,
  formatCommandInvocation,
  getAllCommands,
  parseCommand,
  shouldTriggerPanel,
} from '../../src/composables/useSlashCommands'

function skill(overrides: Partial<WorkshopSkill> = {}): WorkshopSkill {
  return {
    name: 'review',
    description: 'Review changes',
    argumentHint: null,
    version: null,
    source: 'user',
    scope: 'user',
    path: '/home/alice/.agents/skills/review/SKILL.md',
    ...overrides,
  }
}

function command(overrides: Partial<WorkshopCommand> = {}): WorkshopCommand {
  return {
    name: 'deploy',
    description: 'Deploy project',
    argumentHint: null,
    source: 'project',
    scope: 'project',
    path: '/workspace/app/.claude/commands/deploy.md',
    ...overrides,
  }
}

describe('multi-engine composer commands', () => {
  it('accepts both slash and dollar prefixes without triggering inside prose', () => {
    expect(shouldTriggerPanel('/git/pr', 7)).toBe(true)
    expect(shouldTriggerPanel('$browser:inspect', 16)).toBe(true)
    expect(shouldTriggerPanel('run /review', 11)).toBe(false)
    expect(shouldTriggerPanel('/review now', 11)).toBe(false)
  })

  it('normalizes either user prefix to the engine wire protocol', () => {
    const claudeSkills = [skill({ name: 'audit', path: '/home/alice/.claude/skills/audit/SKILL.md' })]
    const claude = parseCommand('$audit src', claudeSkills, [], { engineId: 'claude-code', cwd: '/workspace/app' })
    expect(claude.kind).toBe('pass')
    if (claude.kind === 'pass') expect(formatCommandInvocation(claude.cmd, claude.arg)).toBe('/audit src')

    const codexSkills = [skill({ name: 'audit' })]
    const codex = parseCommand('/audit src', codexSkills, [], { engineId: 'codex', cwd: '/workspace/app' })
    expect(codex.kind).toBe('pass')
    if (codex.kind === 'pass') expect(formatCommandInvocation(codex.cmd, codex.arg)).toBe('$audit src')
  })

  it('applies project over user and the closest repository scope over its parent', () => {
    const skills = [
      skill({ name: 'deploy', scope: 'user', path: '/home/alice/.agents/skills/deploy/SKILL.md' }),
      skill({ name: 'lint', scope: 'repo', path: '/workspace/.agents/skills/lint/SKILL.md' }),
      skill({ name: 'lint', scope: 'repo', path: '/workspace/app/.agents/skills/lint/SKILL.md' }),
    ]
    const commands = [command({ name: 'deploy' })]
    const catalog = getAllCommands(skills, commands, { engineId: 'codex', cwd: '/workspace/app/src' })

    expect(catalog.find(item => item.name === 'deploy')?.path).toBe('/workspace/app/.claude/commands/deploy.md')
    expect(catalog.find(item => item.name === 'lint')?.path).toBe('/workspace/app/.agents/skills/lint/SKILL.md')
  })

  it('keeps built-ins reserved and filters with the typed prefix', () => {
    const catalog = getAllCommands([skill({ name: 'clear', scope: 'project' })], [], { engineId: 'codex' })
    expect(catalog.filter(item => item.name === 'clear')).toHaveLength(1)
    expect(catalog.find(item => item.name === 'clear')?.category).toBe('native')
    expect(filterCommands('$cl', [], [], { engineId: 'codex' }).map(item => item.name)).toContain('clear')
  })
})

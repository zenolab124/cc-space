/**
 * 单会话输入框命令目录。
 *
 * 用户侧同时接受 `/` 与 `$`；命令项通过 wirePrefix 声明引擎真实调用形式：
 * Claude Code 的 skill/command 使用 `/`，Codex skill 使用 `$` + 结构化 skill item。
 */

import { MODELS, MODEL_ALIASES } from '@/utils/modelContext'
import type { WorkshopSkill, WorkshopCommand } from '@/types'
import i18n from '../locales'

export type ComposerPrefix = '/' | '$'
export type SlashCommandCategory = 'native' | 'pass' | 'skill' | 'command' | 'terminal'

export interface SlashCommand {
  name: string
  hint: string
  hasArg: boolean
  argHint?: string
  category: SlashCommandCategory
  wirePrefix: ComposerPrefix
  scope?: string
  path?: string
}

export interface CommandCatalogContext {
  engineId?: string
  cwd?: string | null
}

function commonCommands(): SlashCommand[] {
  const t = (key: string) => i18n.global.t(key)
  return [
    { name: 'new', hint: t('slash.hintNew'), hasArg: false, category: 'native', wirePrefix: '/' },
    { name: 'clear', hint: t('slash.hintClear'), hasArg: false, category: 'native', wirePrefix: '/' },
    { name: 'cd', hint: t('slash.hintCd'), hasArg: true, argHint: '<path>', category: 'native', wirePrefix: '/' },
    { name: 'help', hint: t('slash.hintHelp'), hasArg: false, category: 'native', wirePrefix: '/' },
    { name: 'model', hint: t('slash.hintModel'), hasArg: true, argHint: '<name>', category: 'pass', wirePrefix: '/' },
  ]
}

/** 引擎内置命令；默认 Claude 以兼容旧调用方。 */
export function getBuiltinCommands(engineId = 'claude-code'): SlashCommand[] {
  const t = (key: string) => i18n.global.t(key)
  const common = commonCommands()
  if (engineId !== 'claude-code') return common
  return [
    ...common,
    { name: 'chrome', hint: t('slash.hintChrome'), hasArg: true, argHint: '[on|off]', category: 'pass', wirePrefix: '/' },
    { name: 'compact', hint: t('slash.hintCompact'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'config', hint: t('slash.hintConfig'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'cost', hint: t('slash.hintCost'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'diff', hint: t('slash.hintDiff'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'doctor', hint: t('slash.hintDoctor'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'effort', hint: t('slash.hintEffort'), hasArg: true, argHint: '<level>', category: 'pass', wirePrefix: '/' },
    { name: 'fast', hint: t('slash.hintFast'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'hooks', hint: t('slash.hintHooks'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'init', hint: t('slash.hintInit'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'mcp', hint: t('slash.hintMcp'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'memory', hint: t('slash.hintMemory'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'permissions', hint: t('slash.hintPermissions'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'review', hint: t('slash.hintReview'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'stats', hint: t('slash.hintStats'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'status', hint: t('slash.hintStatus'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'theme', hint: t('slash.hintTheme'), hasArg: true, argHint: '<name>', category: 'pass', wirePrefix: '/' },
    { name: 'undo', hint: t('slash.hintUndo'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'add-dir', hint: t('slash.hintAddDir'), hasArg: true, argHint: '<path>', category: 'pass', wirePrefix: '/' },
    { name: 'resume', hint: t('slash.hintResume'), hasArg: false, category: 'pass', wirePrefix: '/' },
    { name: 'login', hint: t('slash.hintLogin'), hasArg: false, category: 'terminal', wirePrefix: '/' },
    { name: 'logout', hint: t('slash.hintLogout'), hasArg: false, category: 'terminal', wirePrefix: '/' },
    { name: 'terminal-setup', hint: t('slash.hintTerminalSetup'), hasArg: false, category: 'terminal', wirePrefix: '/' },
    { name: 'vim', hint: t('slash.hintVim'), hasArg: false, category: 'terminal', wirePrefix: '/' },
    { name: 'bug', hint: t('slash.hintBug'), hasArg: false, category: 'terminal', wirePrefix: '/' },
    { name: 'ide', hint: t('slash.hintIde'), hasArg: false, category: 'terminal', wirePrefix: '/' },
  ]
}

/** @deprecated 使用 getBuiltinCommands + dynamic 合并。 */
export function getSlashCommands(): SlashCommand[] {
  return getBuiltinCommands()
}

export const SLASH_COMMANDS: SlashCommand[] = getBuiltinCommands()

function shorten(value: string): string {
  return value.length > 60 ? `${value.slice(0, 57)}…` : value
}

function skillToSlash(skill: WorkshopSkill, engineId: string): SlashCommand {
  return {
    name: skill.name,
    hint: shorten(skill.description || ''),
    hasArg: !!skill.argumentHint,
    argHint: skill.argumentHint ?? undefined,
    category: 'skill',
    wirePrefix: engineId === 'codex' ? '$' : '/',
    scope: skill.scope,
    path: skill.path,
  }
}

function commandToSlash(command: WorkshopCommand): SlashCommand {
  return {
    name: command.name,
    hint: shorten(command.description || ''),
    hasArg: !!command.argumentHint,
    argHint: command.argumentHint ?? undefined,
    category: 'command',
    wirePrefix: '/',
    scope: command.scope,
    path: command.path,
  }
}

function normalizedName(name: string): string {
  return name.toLocaleLowerCase()
}

function scopeRank(scope?: string): number {
  switch (scope?.toLowerCase()) {
    case 'project':
    case 'repo': return 4
    case 'user': return 3
    case 'admin': return 2
    case 'system': return 1
    default: return 0
  }
}

function pathSpecificity(command: SlashCommand, cwd?: string | null): number {
  if (!cwd || !command.path || !['project', 'repo'].includes(command.scope?.toLowerCase() ?? '')) return 0
  const normalizedCwd = cwd.replace(/\\/g, '/').replace(/\/+$/, '')
  const normalizedPath = command.path.replace(/\\/g, '/')
  const scopeMarkers = ['/.agents/skills/', '/.claude/skills/', '/.claude/commands/']
  const marker = scopeMarkers.find(value => normalizedPath.includes(value))
  const scopeRoot = marker ? normalizedPath.slice(0, normalizedPath.indexOf(marker)) : normalizedPath
  if (normalizedCwd !== scopeRoot && !normalizedCwd.startsWith(`${scopeRoot}/`)) return 0
  return scopeRoot.split('/').filter(Boolean).length
}

function outranks(candidate: SlashCommand, current: SlashCommand, cwd?: string | null): boolean {
  const candidateScope = scopeRank(candidate.scope)
  const currentScope = scopeRank(current.scope)
  if (candidateScope !== currentScope) return candidateScope > currentScope
  const candidateSpecificity = pathSpecificity(candidate, cwd)
  const currentSpecificity = pathSpecificity(current, cwd)
  if (candidateSpecificity !== currentSpecificity) return candidateSpecificity > currentSpecificity
  // 同层同名保持 Skill 优先，兼容旧目录的确定性行为。
  return candidate.category === 'skill' && current.category === 'command'
}

/**
 * 合并内置与动态来源。内置名为 Monet 保留字；动态项按项目/仓库 > 用户 > 管理员 > 系统覆盖。
 */
export function getAllCommands(
  skills?: WorkshopSkill[],
  commands?: WorkshopCommand[],
  context: CommandCatalogContext = {},
): SlashCommand[] {
  const engineId = context.engineId ?? 'claude-code'
  const builtins = getBuiltinCommands(engineId)
  const reserved = new Set(builtins.map(command => normalizedName(command.name)))
  const dynamic = new Map<string, SlashCommand>()
  const candidates = [
    ...(skills ?? []).map(skill => skillToSlash(skill, engineId)),
    ...(commands ?? []).map(commandToSlash),
  ]
  for (const candidate of candidates) {
    const key = normalizedName(candidate.name)
    if (reserved.has(key)) continue
    const current = dynamic.get(key)
    if (!current || outranks(candidate, current, context.cwd)) dynamic.set(key, candidate)
  }
  return [...builtins, ...dynamic.values()]
}

const KNOWN_CLAUDE_MODELS = new Set([
  ...MODELS.map(model => model.id),
  ...Object.keys(MODEL_ALIASES),
])

function resolveClaudeModelArg(arg: string): string {
  return MODEL_ALIASES[arg] ?? arg
}

const TRIGGER_RE = /^[/$][\p{L}\p{N}_:./-]*$/u

export function composerPrefix(input: string): ComposerPrefix | null {
  return input.startsWith('/') ? '/' : input.startsWith('$') ? '$' : null
}

export function shouldTriggerPanel(input: string, cursorPos: number): boolean {
  if (cursorPos < 1 || cursorPos > input.length) return false
  return TRIGGER_RE.test(input.slice(0, cursorPos))
}

export function filterCommands(
  input: string,
  skills?: WorkshopSkill[],
  commands?: WorkshopCommand[],
  context: CommandCatalogContext = {},
): SlashCommand[] {
  if (!composerPrefix(input)) return []
  const prefix = input.slice(1).toLocaleLowerCase()
  const all = getAllCommands(skills, commands, context)
  if (!prefix) return all
  return all.filter(command => normalizedName(command.name).startsWith(prefix))
}

export type ParsedCommand =
  | { kind: 'unknown'; raw: string }
  | { kind: 'native'; cmd: SlashCommand; arg: string; prefix: ComposerPrefix }
  | { kind: 'pass'; cmd: SlashCommand; arg: string; prefix: ComposerPrefix }
  | { kind: 'terminal'; cmd: SlashCommand; arg: string; prefix: ComposerPrefix }
  | { kind: 'invalid'; cmd: SlashCommand; reason: string; prefix: ComposerPrefix }

export function parseCommand(
  input: string,
  skills?: WorkshopSkill[],
  commands?: WorkshopCommand[],
  context: CommandCatalogContext = {},
): ParsedCommand {
  const raw = input
  const trimmed = input.trim()
  const prefix = composerPrefix(trimmed)
  if (!prefix) return { kind: 'unknown', raw }

  const body = trimmed.slice(1)
  const spaceIdx = body.search(/\s/u)
  const name = normalizedName(spaceIdx === -1 ? body : body.slice(0, spaceIdx))
  const arg = spaceIdx === -1 ? '' : body.slice(spaceIdx + 1).trim()
  const command = getAllCommands(skills, commands, context)
    .find(candidate => normalizedName(candidate.name) === name)
  if (!command) return { kind: 'unknown', raw }

  if (command.name === 'model') {
    if (!arg) {
      return { kind: 'invalid', cmd: command, reason: i18n.global.t('slash.errorModelRequired'), prefix }
    }
    if ((context.engineId ?? 'claude-code') === 'claude-code') {
      const normalized = arg.toLowerCase()
      if (!KNOWN_CLAUDE_MODELS.has(normalized)) {
        return { kind: 'invalid', cmd: command, reason: i18n.global.t('slash.errorModelUnknown'), prefix }
      }
      return { kind: 'pass', cmd: command, arg: resolveClaudeModelArg(normalized), prefix }
    }
    return { kind: 'pass', cmd: command, arg, prefix }
  }

  if (command.name === 'chrome') {
    const normalized = arg.toLowerCase()
    if (normalized !== '' && normalized !== 'on' && normalized !== 'off') {
      return { kind: 'invalid', cmd: command, reason: i18n.global.t('slash.errorChromeArg'), prefix }
    }
    return { kind: 'pass', cmd: command, arg: normalized, prefix }
  }

  if (command.name === 'cd' && !arg) {
    return { kind: 'invalid', cmd: command, reason: i18n.global.t('slash.errorCdRequired'), prefix }
  }
  if (command.category === 'skill' || command.category === 'command') {
    return { kind: 'pass', cmd: command, arg, prefix }
  }
  if (command.category === 'terminal') {
    return { kind: 'terminal', cmd: command, arg, prefix }
  }
  return { kind: command.category, cmd: command, arg, prefix }
}

/** 把用户输入的任一前缀转换为目标引擎真实调用形式。 */
export function formatCommandInvocation(command: SlashCommand, arg: string): string {
  return `${command.wirePrefix}${command.name}${arg ? ` ${arg}` : ''}`
}

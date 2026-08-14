#!/usr/bin/env node

import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const VERSION_PATTERN = /^\d+\.\d+\.\d+$/
const NOTE_TYPES = new Set(['new', 'improved', 'fixed'])
const REQUIRED_LOCALES = ['zh-CN', 'en-US']
const MAX_ITEMS = 20
const MAX_ENCODED_BYTES = 32 * 1024

function requiredText(value, label, maxLength) {
  if (typeof value !== 'string' || !value.trim()) {
    throw new Error(`${label} 必须是非空字符串`)
  }
  const text = value.trim()
  if (text.length > maxLength) {
    throw new Error(`${label} 超过 ${maxLength} 字符`)
  }
  return text
}

function normalizeLocale(value, locale) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`locales.${locale} 必须是对象`)
  }

  if (!Array.isArray(value.items) || value.items.length === 0) {
    throw new Error(`locales.${locale}.items 至少需要一项`)
  }
  if (value.items.length > MAX_ITEMS) {
    throw new Error(`locales.${locale}.items 不能超过 ${MAX_ITEMS} 项`)
  }

  return {
    summary: requiredText(value.summary, `locales.${locale}.summary`, 240),
    items: value.items.map((item, index) => {
      const label = `locales.${locale}.items[${index}]`
      if (!item || typeof item !== 'object' || Array.isArray(item)) {
        throw new Error(`${label} 必须是对象`)
      }
      if (!NOTE_TYPES.has(item.type)) {
        throw new Error(`${label}.type 只支持 new、improved 或 fixed`)
      }
      const normalized = {
        type: item.type,
        title: requiredText(item.title, `${label}.title`, 180),
      }
      if (item.detail != null && item.detail !== '') {
        normalized.detail = requiredText(item.detail, `${label}.detail`, 500)
      }
      return normalized
    }),
  }
}

export function validateReleaseNotes(value, expectedVersion) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('发布说明必须是对象')
  }
  if (value.schema !== 1) {
    throw new Error('发布说明 schema 必须为 1')
  }
  const version = requiredText(value.version, 'version', 32)
  if (!VERSION_PATTERN.test(version)) {
    throw new Error(`发布说明版本号非法: ${version}`)
  }
  if (expectedVersion && version !== expectedVersion) {
    throw new Error(`发布说明版本 ${version} 与目标版本 ${expectedVersion} 不一致`)
  }
  if (!value.locales || typeof value.locales !== 'object' || Array.isArray(value.locales)) {
    throw new Error('locales 必须是对象')
  }

  const locales = Object.fromEntries(
    REQUIRED_LOCALES.map(locale => [locale, normalizeLocale(value.locales[locale], locale)]),
  )
  const zhTypes = locales['zh-CN'].items.map(item => item.type)
  const enTypes = locales['en-US'].items.map(item => item.type)
  if (zhTypes.length !== enTypes.length || zhTypes.some((type, index) => type !== enTypes[index])) {
    throw new Error('zh-CN 与 en-US 的条目数量和类型顺序必须一致')
  }

  const normalized = { schema: 1, version, locales }
  const encoded = JSON.stringify(normalized)
  if (Buffer.byteLength(encoded, 'utf8') > MAX_ENCODED_BYTES) {
    throw new Error(`发布说明编码后不能超过 ${MAX_ENCODED_BYTES} 字节`)
  }
  return normalized
}

export function readReleaseNotes(path, expectedVersion) {
  let parsed
  try {
    parsed = JSON.parse(readFileSync(path, 'utf8'))
  } catch (error) {
    throw new Error(`无法读取发布说明 ${path}: ${error.message}`)
  }
  return validateReleaseNotes(parsed, expectedVersion)
}

export function encodeReleaseNotes(value, expectedVersion) {
  return JSON.stringify(validateReleaseNotes(value, expectedVersion))
}

const SECTION_LABELS = {
  'zh-CN': { heading: '本次更新', new: '新增', improved: '改进', fixed: '修复' },
  'en-US': { heading: "What's new in Monet", new: 'New', improved: 'Improved', fixed: 'Fixed' },
}

export function renderReleaseNotesMarkdown(value, expectedVersion) {
  const notes = validateReleaseNotes(value, expectedVersion)
  const output = []
  for (const locale of REQUIRED_LOCALES) {
    const labels = SECTION_LABELS[locale]
    const content = notes.locales[locale]
    output.push(`## ${labels.heading}`, '', content.summary, '')
    for (const type of NOTE_TYPES) {
      const items = content.items.filter(item => item.type === type)
      if (!items.length) continue
      output.push(`### ${labels[type]}`, '')
      for (const item of items) {
        output.push(`- ${item.title}${item.detail ? ` — ${item.detail}` : ''}`)
      }
      output.push('')
    }
  }
  return `${output.join('\n').trim()}\n`
}

export function createNightlyReleaseNotes(version, subjects) {
  const cleanSubjects = [...new Set(subjects
    .map((subject) => {
      const title = subject.trim()
      if (title.length <= 180) return title
      let truncated = ''
      for (const character of title) {
        if ((truncated + character).length > 179) break
        truncated += character
      }
      return `${truncated}…`
    })
    .filter(Boolean))].slice(0, 8)
  const items = (cleanSubjects.length ? cleanSubjects : ['Nightly build refresh']).map(title => ({
    type: 'improved',
    title,
  }))
  return validateReleaseNotes({
    schema: 1,
    version,
    locales: {
      'zh-CN': {
        summary: '每日构建，包含自上次 Nightly 以来的最新改动，尚未经过稳定版完整验证。',
        items,
      },
      'en-US': {
        summary: 'Daily build with changes since the previous Nightly; not yet fully validated for Stable.',
        items,
      },
    },
  }, version)
}

function usage() {
  console.error('usage: release-notes.mjs validate <file> <version> | markdown <file> <version> | nightly <version> <subjects-file> <out>')
  process.exit(1)
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  const [command, ...args] = process.argv.slice(2)
  if (command === 'validate' && args.length === 2) {
    readReleaseNotes(args[0], args[1])
    console.log(`发布说明校验通过: ${args[0]}`)
  } else if (command === 'markdown' && args.length === 2) {
    process.stdout.write(renderReleaseNotesMarkdown(readReleaseNotes(args[0], args[1]), args[1]))
  } else if (command === 'nightly' && args.length === 3) {
    const [version, subjectsPath, outPath] = args
    const subjects = readFileSync(subjectsPath, 'utf8').split(/\r?\n/)
    const notes = createNightlyReleaseNotes(version, subjects)
    writeFileSync(outPath, `${JSON.stringify(notes, null, 2)}\n`)
    console.log(`Nightly 发布说明 → ${outPath}`)
  } else {
    usage()
  }
}

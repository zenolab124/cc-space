#!/usr/bin/env node

import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const VERSION_PATTERN = /^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/

function parseVersion(value) {
  const match = VERSION_PATTERN.exec(value)
  if (!match) throw new Error(`非法版本号: ${value}`)
  return match.slice(1, 4).map(Number)
}

function compareVersion(left, right) {
  for (let index = 0; index < 3; index += 1) {
    if (left[index] !== right[index]) return left[index] - right[index]
  }
  return 0
}

/**
 * 正式版和 Nightly 共用同一递增序列；候选可含 prerelease，比较时取其数字核心。
 */
export function nextBuildVersion(bump, candidates) {
  if (!['patch', 'minor', 'major'].includes(bump)) throw new Error(`不支持的升级类型: ${bump}`)
  if (!candidates.length) throw new Error('至少需要一个已有版本号')

  const current = candidates.map(parseVersion).reduce((highest, version) => (
    compareVersion(version, highest) > 0 ? version : highest
  ))
  const [major, minor, patch] = current

  if (bump === 'major') return `${major + 1}.0.0`
  if (bump === 'minor') return `${major}.${minor + 1}.0`
  return `${major}.${minor}.${patch + 1}`
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : null
if (invokedPath === import.meta.url) {
  try {
    const [bump = 'patch', ...candidates] = process.argv.slice(2)
    console.log(nextBuildVersion(bump, candidates))
  } catch (error) {
    console.error(error.message)
    process.exit(1)
  }
}

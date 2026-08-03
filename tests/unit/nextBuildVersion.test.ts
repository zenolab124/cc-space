import { describe, expect, it } from 'vitest'

import { nextBuildVersion } from '../../scripts/next-build-version.mjs'

describe('nextBuildVersion', () => {
  it('在正式版与 Nightly 中取全局最大 patch', () => {
    expect(nextBuildVersion('patch', ['1.0.4', '1.0.5-nightly.20260803.10'])).toBe('1.0.6')
  })

  it('连续构建继续消耗后续版本号', () => {
    expect(nextBuildVersion('patch', ['1.0.4', '1.0.6'])).toBe('1.0.7')
  })

  it('支持显式 minor 与 major 发布', () => {
    expect(nextBuildVersion('minor', ['1.9.8', '1.9.9-nightly'])).toBe('1.10.0')
    expect(nextBuildVersion('major', ['1.9.9', '2.0.0-nightly'])).toBe('3.0.0')
  })

  it('拒绝非法版本号和升级类型', () => {
    expect(() => nextBuildVersion('patch', ['nightly'])).toThrow('非法版本号')
    expect(() => nextBuildVersion('build', ['1.0.0'])).toThrow('不支持的升级类型')
  })
})

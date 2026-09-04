import { describe, expect, it, vi } from 'vitest'
import { resolveAsset } from '../../src/engines/client'
import { resolveAssetLimited } from '../../src/engines/assetLoader'
import type { SessionRef } from '../../src/engines/types'

vi.mock('../../src/engines/client', () => ({
  resolveAsset: vi.fn(),
}))

describe('engine asset loader', () => {
  it('keeps at most four asset requests in flight', async () => {
    const pending: Array<(value: { mediaType: string, bytes: number[] }) => void> = []
    vi.mocked(resolveAsset).mockImplementation(() => new Promise(resolve => pending.push(resolve)))
    const session: SessionRef = {
      engine: { engineId: 'fixture', instanceId: 'default' },
      nativeId: 'session',
    }

    const loads = Array.from({ length: 6 }, (_, index) =>
      resolveAssetLimited(session, `asset-${index}`, true),
    )
    await Promise.resolve()
    expect(resolveAsset).toHaveBeenCalledTimes(4)

    pending.shift()?.({ mediaType: 'image/png', bytes: [] })
    await vi.waitFor(() => expect(resolveAsset).toHaveBeenCalledTimes(5))

    while (pending.length) {
      pending.shift()?.({ mediaType: 'image/png', bytes: [] })
      await Promise.resolve()
    }
    await Promise.all(loads)
    expect(resolveAsset).toHaveBeenCalledTimes(6)
  })
})

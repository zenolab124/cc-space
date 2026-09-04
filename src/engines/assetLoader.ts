import { resolveAsset } from './client'
import type { SessionRef } from './types'

const MAX_CONCURRENT_ASSET_LOADS = 4

let activeLoads = 0
const waiters: Array<() => void> = []

async function acquireSlot(): Promise<void> {
  if (activeLoads < MAX_CONCURRENT_ASSET_LOADS) {
    activeLoads += 1
    return
  }
  await new Promise<void>(resolve => waiters.push(resolve))
}

function releaseSlot() {
  const next = waiters.shift()
  if (next) next()
  else activeLoads = Math.max(0, activeLoads - 1)
}

export async function resolveAssetLimited(
  session: SessionRef,
  nativeId: string,
  preview = false,
) {
  await acquireSlot()
  try {
    return await resolveAsset(session, nativeId, preview)
  } finally {
    releaseSlot()
  }
}

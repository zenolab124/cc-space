import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

function section(text: string, marker: string, length = 2600): string {
  const start = text.indexOf(marker)
  expect(start, `missing source marker: ${marker}`).toBeGreaterThanOrEqual(0)
  return text.slice(start, start + length)
}

describe('scroll follow race guards', () => {
  it('invalidates pending Runner log scrolls as soon as upward reading begins', () => {
    const runner = source('../../src/components/runner/RunnerLogView.vue')
    const wheel = section(runner, 'function onLogWheel')
    const scroll = section(runner, 'function onScroll')
    const request = section(runner, 'function requestScrollToEnd')

    expect(runner).toContain('const followLogs = ref(true)')
    expect(runner).toContain('@wheel.passive="onLogWheel"')
    expect(wheel).toMatch(/event\.deltaY < 0[\s\S]*stopFollowingLogs\(\)/)
    expect(scroll).toMatch(/delta < -0\.5[\s\S]*stopFollowingLogs\(\)/)
    expect(scroll).toContain('downwardIntentAt > upwardIntentAt')
    expect(request).toContain('const generation = scrollFollowGeneration')
    expect(request).toContain('const requestId = ++scrollRequestId')
    expect(request).toContain('void nextTick(() =>')
    expect(request).toContain('requestAnimationFrame(() =>')
    expect(request).toContain('generation !== scrollFollowGeneration')
    expect(request).toContain('requestId !== scrollRequestId')
    expect(request).toContain('el.scrollTop < scheduledScrollTop - 0.5')
    expect(request).toContain('distanceFromBottom(el) > BOTTOM_THRESHOLD')
    expect(request.indexOf('generation !== scrollFollowGeneration'))
      .toBeLessThan(request.indexOf('el.scrollTop = el.scrollHeight'))
    expect(runner).toMatch(/watch\(\(\) => filteredLines\.value\.length,[\s\S]*if \(followLogs\.value\) requestScrollToEnd\(\)/)
  })

  it('keeps Runner virtual measurements keyed by runner and suppresses first-measure fights while scrolling up', () => {
    const runner = source('../../src/components/runner/RunnerLogView.vue')
    const virtualizer = section(runner, 'const virtualizer = useVirtualizer')
    const adjustment = section(runner, 'virtualizer.value.shouldAdjustScrollPositionOnItemSizeChange')

    expect(runner).toContain('const virtualLineKeySnapshot = shallowRef<readonly string[]>([])')
    expect(runner).toContain('keys.every((key, index) => key === previous[index])')
    expect(virtualizer).toContain('getItemKey: virtualLineKeyExtractor.value')
    expect(adjustment).toContain('shouldCompensateVirtualItemSizeChange')
    expect(adjustment).toContain('performance.now() - upwardIntentAt < 220')
    expect(adjustment).toContain('scrollDirection: instance.scrollDirection')
    expect(adjustment).toContain('itemSize: item.size')
    expect(runner).toContain(':key="String(vRow.key)"')
    expect(runner).not.toContain(':key="vRow.index"')
  })

  it('keeps the primary session detached across stream settle and invalidates every delayed bottom write', () => {
    const session = source('../../src/components/SessionDetail.vue')
    const wheel = section(session, 'function onScrollWheel')
    const bottom = section(session, 'function scrollToBottom', 3600)
    const settle = section(session, 'const settlingSessions', 2600)

    expect(wheel).toMatch(/e\.deltaY < 0|e\.deltaY >= 0/)
    expect(wheel).toContain('detachFollow()')
    expect(bottom).toContain('const requestId = ++scrollBottomRequestId')
    expect(bottom).toContain('const scheduledScrollTop = scheduledElement?.scrollTop ?? 0')
    expect(bottom).toContain('const userMovedUp = (element: HTMLElement)')
    expect(bottom).toContain('detachFollow()')
    expect(bottom).toContain('isScrollGenerationCurrent(token.epoch, sessionId)')
    expect(bottom).toContain('canApplyScrollFollowToken(scrollFollowState, token)')
    expect(bottom).toContain('void nextTick(() =>')
    expect(bottom).toContain('requestAnimationFrame(() =>')
    expect(settle).toContain('preserveFollowAfterStreamFinished()')
    expect(settle).not.toContain('followStreaming.value = true')
  })

  it('hands stable keyed heights from the direct last group into the virtualizer', () => {
    const session = source('../../src/components/SessionDetail.vue')
    const virtualizer = section(session, 'const messageVirtualizer = useVirtualizer', 2600)
    const handoff = section(session, 'function rememberRenderedGroupHeights', 1800)

    expect(virtualizer).toContain('getItemKey: groupKeyExtractor.value')
    expect(virtualizer).toContain('groupHeightEstimates.get')
    expect(virtualizer).toContain('shouldAdjustScrollPositionOnItemSizeChange')
    expect(virtualizer).toContain('shouldCompensateVirtualItemSizeChange')
    expect(session).toContain('const groupKeySnapshot = shallowRef<readonly string[]>([])')
    expect(session).toContain('keys.every((key, index) => key === previous[index])')
    expect(handoff).toContain("querySelectorAll<HTMLElement>('[data-group-key]')")
    expect(handoff).toContain('groupHeightEstimates.set(key, height)')
    expect(handoff).toMatch(/watch\(renderGroupKeys,[\s\S]*keys\.length > previous\.length[\s\S]*pinLastGroupBeforeSwap\(\)/)
    expect(session).toContain(':key="String(vitem.key)"')
    expect(session).toContain(':data-group-key="vitem.key"')
  })

  it('revalidates standard-engine timeline follow after nextTick and animation frame', () => {
    const engine = source('../../src/components/engine/EngineSessionDetail.vue')
    const wheel = section(engine, 'function onTimelineWheel')
    const scroll = section(engine, 'function onTimelineScroll')
    const request = section(engine, 'function requestTimelineFollow')

    expect(engine).toContain('@wheel="onTimelineWheel"')
    expect(wheel).toMatch(/event\.deltaY < 0[\s\S]*stopTimelineFollow\(\)/)
    expect(scroll).toMatch(/delta < -0\.5[\s\S]*stopTimelineFollow\(\)/)
    expect(scroll).toContain('timelineDownwardIntentAt > timelineUpwardIntentAt')
    expect(scroll).toContain('performance.now() - timelineDownwardIntentAt <= TIMELINE_SCROLL_INTENT_MS')
    expect(request).toContain('const generation = timelineFollowGeneration')
    expect(request).toContain('const requestId = ++timelineScrollRequestId')
    expect(request).toContain('void nextTick(() =>')
    expect(request).toContain('requestAnimationFrame(() =>')
    expect(request).toContain('generation !== timelineFollowGeneration')
    expect(request).toContain('requestId !== timelineScrollRequestId')
    expect(request).toContain('element.scrollTop < scheduledScrollTop - 0.5')
    expect(request).toContain('timelineDistanceFromBottom(element) > TIMELINE_BOTTOM_THRESHOLD')
    expect(request.indexOf('generation !== timelineFollowGeneration'))
      .toBeLessThan(request.indexOf('element.scrollTop = target'))
  })

  it('follows same-record streaming growth through ResizeObserver without direct watcher scroll writes', () => {
    const engine = source('../../src/components/engine/EngineSessionDetail.vue')

    expect(engine).toContain('timelineResizeObserver = new ResizeObserver(() => requestTimelineFollow())')
    expect(engine).toContain('ref="timelineContentElement"')
    expect(engine).toMatch(/watch\(\(\) => allRecords\.value\.length, \(\) => \{\s*requestTimelineFollow\(\)/)
    expect(engine).not.toMatch(/watch\(\(\) => allRecords\.value\.length,[\s\S]{0,180}scrollTo\(/)
    expect(engine).toMatch(/watch\(\(\) => props\.session\.id,[\s\S]{0,260}resetTimelineFollow\(\)/)
    expect(engine).toMatch(/onUnmounted\(\(\) => \{[\s\S]*invalidateTimelineScrollRequests\(\)[\s\S]*timelineResizeObserver\?\.disconnect\(\)/)
  })
})

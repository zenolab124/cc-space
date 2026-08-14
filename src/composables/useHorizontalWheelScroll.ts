import { onMounted, onUnmounted, type Ref } from 'vue'
import { registerScrollSurface } from '@/lib/scrollGestureCoordinator'

/**
 * 工作台横向滚动接管（嵌套滚动 wheel 捕获，动机见 pitfalls/nested-scroll-wheel-capture.md）。
 *
 * 性能约束（docs/research/perf-audit-2026-07.md · P0-1 / 顿挫治理）：
 * - deltaX 判断前置：纵向滚动事件（绝大多数）不读任何 layout 属性即返回，
 *   WebKit 手势级快速路径得以保留
 * - 速度模型平滑：wheel 只更新目标位置，rAF 循环用帧率校正的指数平滑逼近目标。
 *   JS 驱动滚动的固有弱点是主线程微忙（流式 tick、GC）时 rAF 迟到、该帧位移缺失，
 *   肉眼即"顿一下"；插值模型下迟到帧的下一帧自动以更大步长追赶，视觉连续。
 *   收敛尾段自带轻微惯性，替代原生 momentum 的缺失
 * - 放行内层可横滚元素（代码块等横向溢出区），判定按 target 缓存：
 *   指针不动的连续滚动零 layout 读；元素被 v-html 重建替换时因身份变化自动重算。
 *   已知权衡：同一元素的溢出状态中途改变（流式追加）时判定短暂陈旧，指针移动后自愈
 */
export function useHorizontalWheelScroll(containerRef: Ref<HTMLElement | undefined>) {
  /** 目标位置（clamp 到手势起点缓存的 maxScroll） */
  let target = 0
  /** 插值当前位置 */
  let current = 0
  /** 手势起点缓存的最大滚动距离——插值循环全程零布局读（见 step 注释） */
  let maxScroll = 0
  let animating = false
  let rafId = 0
  let lastTs = 0
  let lastTarget: EventTarget | null = null
  let lastAllowInner = false
  let unregisterScrollSurface: (() => void) | null = null

  // 60fps 基准下每帧吃掉剩余距离的比例。0.45 偏跟手（用户校准，0.35 感知滞后）；
  // 帧间隔变化时按 dt 指数校正，追赶速度与刷新率无关
  const LERP_BASE = 0.45

  function step(ts: number) {
    const el = containerRef.value
    if (!el) {
      animating = false
      return
    }
    const t0 = performance.now()
    const dt = lastTs > 0 ? ts - lastTs : 16.7
    lastTs = ts
    // 铁律：本循环内不读任何布局属性（scrollWidth/clientWidth/offsetLeft…）。
    // 流式期间布局树几乎每帧是脏的，读取即强制全量同步 layout（实测 70ms+ 尖峰，
    // 正是 P0-1 审计问题在 rAF 里的复活）。clamp 用手势起点缓存的 maxScroll，
    // 手势中内容尺寸变化的误差由浏览器对 scrollLeft 写入的自动钳制兜底
    if (target < 0) target = 0
    if (target > maxScroll) target = maxScroll
    const remain = target - current
    if (Math.abs(remain) < 0.5) {
      current = target
      el.scrollLeft = current
      animating = false
    } else {
      const alpha = 1 - Math.pow(1 - LERP_BASE, dt / 16.7)
      current += remain * alpha
      el.scrollLeft = current
      rafId = requestAnimationFrame(step)
    }
    performance.measure('hscroll-step', { start: t0, duration: performance.now() - t0 })
  }

  function startAnim() {
    if (animating) return
    animating = true
    lastTs = 0
    rafId = requestAnimationFrame(step)
  }

  /** target 到容器之间是否存在可横向滚动的祖先（如 pre 代码块） */
  function hasInnerHScroll(start: Element | null, boundary: HTMLElement): boolean {
    for (let node = start; node && node !== boundary; node = node.parentElement) {
      if (node.scrollWidth > node.clientWidth + 1) {
        const overflowX = getComputedStyle(node).overflowX
        if (overflowX === 'auto' || overflowX === 'scroll') return true
      }
    }
    return false
  }

  function onWheelCapture(e: WheelEvent) {
    if (Math.abs(e.deltaX) <= Math.abs(e.deltaY)) return
    const el = containerRef.value
    if (!el) return
    if (e.target !== lastTarget) {
      lastTarget = e.target
      lastAllowInner = hasInnerHScroll(e.target as Element, el)
    }
    if (lastAllowInner) return
    e.preventDefault()
    queueDelta(e.deltaX)
  }

  function queueDelta(delta: number) {
    const el = containerRef.value
    if (!el) return
    if (!animating) {
      // 手势起点与真实位置对齐并缓存滚动上限（整个手势期唯一的布局读时机）：
      // scrollLeft 可能被外部改过（聚焦列 scrollIntoView 等）
      current = el.scrollLeft
      target = current
      maxScroll = Math.max(0, el.scrollWidth - el.clientWidth)
    }
    target += delta
    startAnim()
  }

  onMounted(() => {
    const element = containerRef.value
    element?.addEventListener('wheel', onWheelCapture, { capture: true, passive: false })
    if (element) unregisterScrollSurface = registerScrollSurface(element, 'x', queueDelta)
  })

  onUnmounted(() => {
    containerRef.value?.removeEventListener('wheel', onWheelCapture, { capture: true } as EventListenerOptions)
    unregisterScrollSurface?.()
    unregisterScrollSurface = null
    if (rafId) cancelAnimationFrame(rafId)
    animating = false
    lastTarget = null
  })
}

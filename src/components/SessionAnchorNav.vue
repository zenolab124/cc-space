<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'

export interface AnchorItem {
  index: number
  text: string
}

const props = defineProps<{
  anchors: AnchorItem[]
  scrollContainer: HTMLElement | undefined
  /** 虚拟化感知回调:目标可能不在虚拟窗口内,先由父组件用 virtualizer 滚过去,再 rAF 精调。
   *  未传时按原路径直接找 DOM(小会话无虚拟化时行为不变) */
  onScrollToIndex?: (index: number) => void
}>()

const activeIndex = ref(-1)
let observer: IntersectionObserver | null = null

const railRef = ref<HTMLElement>()
const railHeight = ref(0)
let resizeObs: ResizeObserver | null = null

const MAX_WRAP = 12
const MIN_WRAP = 8

const layout = computed(() => {
  const n = props.anchors.length
  if (n <= 1) return { wrap: MAX_WRAP }
  const avail = railHeight.value // contentRect 已排除 padding
  if (avail <= 0 || n * MAX_WRAP <= avail) return { wrap: MAX_WRAP }
  // 锚点命中区首尾相接：空间不足时只压缩单段高度，不再制造交互断层。
  return { wrap: Math.max(MIN_WRAP, Math.floor(avail / n)) }
})

onMounted(() => {
  resizeObs = new ResizeObserver(([e]) => { railHeight.value = e.contentRect.height })
  if (railRef.value) resizeObs.observe(railRef.value)
})
watch(railRef, (el) => { if (el && resizeObs) resizeObs.observe(el) })
onUnmounted(() => resizeObs?.disconnect())

function resolveEl(index: number): HTMLElement | null {
  return props.scrollContainer?.querySelector<HTMLElement>(`[data-anchor-index="${index}"]`) ?? null
}

function setupObserver() {
  observer?.disconnect()
  if (!props.scrollContainer || !props.anchors.length) return

  observer = new IntersectionObserver(
    (entries) => {
      let topMost: { index: number; top: number } | null = null
      for (const entry of entries) {
        if (!entry.isIntersecting) continue
        const idx = Number(entry.target.getAttribute('data-anchor-index'))
        if (isNaN(idx)) continue
        const top = entry.boundingClientRect.top
        if (!topMost || top < topMost.top) topMost = { index: idx, top }
      }
      if (topMost) activeIndex.value = topMost.index
    },
    { root: props.scrollContainer, threshold: 0, rootMargin: '0px 0px -70% 0px' },
  )

  for (const a of props.anchors) {
    const el = resolveEl(a.index)
    if (el) observer.observe(el)
  }
}

watch(() => [props.anchors, props.scrollContainer] as const, () => {
  nextTick(setupObserver)
}, { flush: 'post' })

onUnmounted(() => observer?.disconnect())

function scrollTo(anchor: AnchorItem) {
  if (!props.scrollContainer) return
  // 三层收敛:先给虚拟化父组件滚过去(如果传了回调),再 rAF×2 精调 offsetTop
  props.onScrollToIndex?.(anchor.index)
  const smooth = () => {
    const el = resolveEl(anchor.index)
    if (!el || !props.scrollContainer) return
    props.scrollContainer.scrollTo({ top: el.offsetTop, behavior: 'smooth' })
  }
  const immediate = resolveEl(anchor.index)
  if (immediate && !props.onScrollToIndex) {
    // 未传回调路径:直接同步滚(行为等价原实现)
    smooth()
    return
  }
  requestAnimationFrame(() => requestAnimationFrame(smooth))
}

const hoveredIndex = ref(-1)
const hoverPos = ref({ x: 0, y: 0 })

function onDotEnter(e: MouseEvent, index: number) {
  hoveredIndex.value = index
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  hoverPos.value = { x: rect.right + 6, y: rect.top + rect.height / 2 }
}

const showNav = computed(() => props.anchors.length > 1)
</script>

<template>
  <div v-if="showNav" ref="railRef" class="anchor-rail">
    <div
      v-for="a in anchors"
      :key="a.index"
      class="anchor-dot-wrap"
      :style="{ width: layout.wrap + 'px', height: layout.wrap + 'px' }"
      @mouseenter="onDotEnter($event, a.index)"
      @mouseleave="hoveredIndex = -1"
      @click="scrollTo(a)"
    >
      <div
        class="anchor-dot"
        :class="{ active: activeIndex === a.index }"
      />
    </div>
  </div>
  <Teleport to="body">
    <Transition name="anchor-tip">
      <div
        v-if="hoveredIndex >= 0"
        class="anchor-tooltip"
        :style="{ left: hoverPos.x + 'px', top: hoverPos.y + 'px' }"
      >
        {{ anchors.find(a => a.index === hoveredIndex)?.text }}
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.anchor-rail {
  position: absolute;
  left: 7px;
  top: 0;
  bottom: 0;
  z-index: 20;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 12px 0;
  width: 20px;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: none;
  pointer-events: none;
}
.anchor-rail::-webkit-scrollbar { display: none; }

.anchor-dot-wrap {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  flex-shrink: 0;
  pointer-events: auto;
}

.anchor-dot {
  width: 2px;
  height: 8px;
  border-radius: 1px;
  background: var(--primary);
  opacity: 0;
  transform: scaleX(1);
  transition: transform 140ms ease, opacity 120ms ease;
}
.anchor-dot-wrap:hover .anchor-dot {
  opacity: 0.72;
  transform: scaleX(2.5);
}
.anchor-dot.active {
  opacity: 1;
  background: var(--primary);
  width: 6px;
  height: 6px;
  border-radius: 50%;
  transform: none;
}
.anchor-dot-wrap:hover .anchor-dot.active {
  opacity: 1;
  transform: scale(1.18);
}

@media (prefers-reduced-motion: reduce) {
  .anchor-dot { transition: none; }
}
</style>

<style>
.anchor-tooltip {
  position: fixed;
  transform: translateY(-50%);
  background: var(--popover);
  border: 1px solid var(--border);
  color: var(--popover-foreground);
  font-size: 12px;
  line-height: 1.5;
  padding: 6px 10px;
  border-radius: 6px;
  white-space: nowrap;
  max-width: 280px;
  overflow: hidden;
  text-overflow: ellipsis;
  z-index: 9999;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  pointer-events: none;
}

.anchor-tip-enter-active { transition: opacity 0.12s ease, transform 0.12s ease; }
.anchor-tip-leave-active { transition: opacity 0.08s ease; }
.anchor-tip-enter-from { opacity: 0; transform: translateY(-50%) translateX(-4px); }
.anchor-tip-leave-to { opacity: 0; transform: translateY(-50%); }
</style>

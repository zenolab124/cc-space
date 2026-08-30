<script setup lang="ts">
import { ref, computed, inject, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import type { ContentBlock } from '@/types'
import { renderMarkdownPlain, renderMarkdownCached, renderMarkdownDeferred } from '@/composables/useMarkdown'
import { useNotifications } from '@/composables/useNotifications'
import { SESSION_FILE_FALLBACK_ROOT, SESSION_FILE_ROOT } from '@/composables/useSessionFileLinks'
import { openExternalUrl } from '@/composables/useFileOpener'
import { normalizeLocalFileLink } from '@/features/artifact-preview/detectArtifacts'
import { createStreamSplitter } from '@/lib/stream-markdown/findSafeSplit'
import { useStreamSegments } from '@/composables/useStreamSegments'
import MdSegment from './MdSegment.vue'
import { TEXT_TRUNCATE_LEN, persistKeyOf } from '@/lib/stream-markdown/constants'

// FR-008 渲染路径开关(开发/救急,非用户功能):blocks(默认)|legacy。
// setup 读一次,切换需刷新;legacy 分支保留一个发版周期后连同本常量删除
const RENDERER: 'blocks' | 'legacy' =
  localStorage.getItem('monet-stream-renderer') === 'legacy' ? 'legacy' : 'blocks'

const props = defineProps<{
  block: Extract<ContentBlock, { type: 'text' }>
  streaming?: boolean
}>()

const { t } = useI18n()
const { notifyTransient } = useNotifications()
const sessionFileRoot = inject(SESSION_FILE_ROOT, null)
const sessionFileFallbackRoot = inject(SESSION_FILE_FALLBACK_ROOT, null)

const expanded = ref(false)
const isLargeText = computed(() => props.block.text.length > TEXT_TRUNCATE_LEN)
const displayText = computed(() => {
  if (expanded.value || !isLargeText.value) return props.block.text
  return props.block.text.slice(0, TEXT_TRUNCATE_LEN)
})

// 模式单向(FR-002):渲染路径在块出生时钉死,终身不翻转——
// 流式出生 → 段数组路径(含流式结束后);历史出生 → cached 单路径。
// 「streaming→static 全量重渲染」这一事件在新路径下物理上不存在。
const bornStreaming = props.streaming === true && RENDERER === 'blocks'

// —— 段数组路径(流式出生) ——
const segApi = bornStreaming
  ? useStreamSegments({
      text: () => displayText.value,
      streaming: () => props.streaming === true,
      // 预热 key 必须与历史出生路径 renderMarkdownCached(displayText) 的初始入参逐字节一致:
      // 历史区初始 expanded=false,大文本渲染的是截断串
      persistText: () => persistKeyOf(props.block.text),
    })
  : null
const segments = segApi?.segments ?? []
const tailSource = segApi?.tailSource ?? ref('')
const tailColored = segApi?.tailColored ?? ref<string | undefined>(undefined)
const atomicSource = segApi?.atomicSource ?? ref<string | undefined>(undefined)
const atomicColored = segApi?.atomicColored ?? ref<string | undefined>(undefined)

// 展开/折叠切换文本非前缀变化,段状态全量重建(FR-002 边界③,低频允许整块重渲)
watch(expanded, () => segApi?.rebuild())

// —— cached 单路径(历史出生):行为与 v2.4.x 完全一致 ——
const staticHtml = computed(() =>
  bornStreaming || RENDERER === 'legacy' ? '' : renderMarkdownCached(displayText.value),
)

// —— legacy 路径(FR-008 回退分支):沿用单容器输出,但分割仍复用 HTML 安全守卫 ——
function legacyFindSafeSplit(text: string): number {
  const points = createStreamSplitter().update(text)
  return points[points.length - 1] ?? -1
}
const MIN_STABLE_LEN = 200
const legacyStableHtml = ref('')
const legacyStableLen = ref(0)
const legacyDeferredHtml = ref('')
const legacyWasStreaming = ref(false)
if (RENDERER === 'legacy') {
  watch(() => (props.streaming ? displayText.value : null), text => {
    if (!text) return
    const split = legacyFindSafeSplit(text)
    if (split > legacyStableLen.value && split >= MIN_STABLE_LEN) {
      legacyStableHtml.value = renderMarkdownPlain(text.slice(0, split))
      legacyStableLen.value = split
    }
  })
  watch(() => props.streaming, (now, was) => {
    if (was && !now) {
      legacyWasStreaming.value = true
      renderMarkdownDeferred(displayText.value).then(html => {
        legacyDeferredHtml.value = html
        legacyStableHtml.value = ''
        legacyStableLen.value = 0
      })
    }
  })
}
const legacyHtml = computed(() => {
  const pendingShiki = legacyWasStreaming.value && !legacyDeferredHtml.value
  if (props.streaming || pendingShiki) {
    const text = displayText.value
    if (legacyStableLen.value > 0) {
      const tail = text.slice(legacyStableLen.value)
      return legacyStableHtml.value + (tail ? renderMarkdownPlain(tail) : '')
    }
    return renderMarkdownPlain(text)
  }
  if (legacyWasStreaming.value) return legacyDeferredHtml.value
  return renderMarkdownCached(displayText.value)
})

async function openMarkdownLink(href: string) {
  try {
    if (/^https?:\/\//i.test(href) || href.startsWith('//')) {
      await openExternalUrl(href.startsWith('//') ? `https:${href}` : href)
      return
    }
    const path = normalizeLocalFileLink(href)
    const root = sessionFileRoot?.value
    if (!path || !root) throw new Error(t('artifactPreview.fileLinkUnavailable'))
    await invoke('open_local_file', {
      root,
      path,
      fallbackRoot: sessionFileFallbackRoot?.value ?? null,
    })
  } catch (cause) {
    notifyTransient(t('common.openFailed'), String(cause))
  }
}

function onProseClick(e: MouseEvent) {
  const target = e.target as HTMLElement
  const anchor = target.closest<HTMLAnchorElement>('a[href]')
  if (anchor) {
    const href = anchor.getAttribute('href')?.trim()
    if (!href || href.startsWith('#')) return
    e.preventDefault()
    e.stopPropagation()
    void openMarkdownLink(href)
    return
  }

  const btn = target.closest('.code-copy-btn')
  if (btn) {
    e.preventDefault()
    const pre = btn.closest('.code-block-wrapper')?.querySelector('pre')
    if (!pre) return
    navigator.clipboard.writeText(pre.textContent ?? '').then(() => {
      btn.setAttribute('data-copied', '')
      setTimeout(() => btn.removeAttribute('data-copied'), 1500)
    })
  }
}
</script>

<template>
  <div class="prose-msg message-prose" @click="onProseClick">
    <!-- 段数组路径:冻结段索引 key(内容 hash 会致 remount 闪烁),tail 固定 key 独立渲染位 -->
    <template v-if="bornStreaming && atomicSource === undefined">
      <MdSegment v-for="(s, i) in segments" :key="i" :source="s.source" :colored="s.colored" />
      <MdSegment key="tail" :source="tailSource" :colored="tailColored" />
    </template>
    <!-- 块级 HTML 必须保持单根,不能把标签拆到多个 v-html 子树中。 -->
    <MdSegment
      v-else-if="bornStreaming"
      key="atomic"
      :source="atomicSource ?? ''"
      :colored="atomicColored"
    />
    <div v-else-if="RENDERER === 'legacy'" v-html="legacyHtml" />
    <div v-else v-html="staticHtml" />
    <button
      v-if="isLargeText"
      class="text-xs text-primary hover:text-primary/80 ml-1"
      @click="expanded = !expanded"
    >
      {{ expanded ? $t('common.collapse') : $t('common.expandAll', { size: Math.round(block.text.length / 1024) }) }}
    </button>
  </div>
</template>

<style scoped>
.prose-msg.message-prose {
  font-size: 13px;
  line-height: 1.72;
  letter-spacing: 0.006em;
}
.message-prose :deep(code),
.message-prose :deep(pre) {
  letter-spacing: normal;
}
</style>

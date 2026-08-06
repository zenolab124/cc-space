// bootTrace 必须是首个 import(零依赖):其 body 的 boot:js-start 标记模块图执行起点
import { markBoot, finishBootTrace } from './utils/bootTrace'
import { createApp } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import 'virtual:uno.css'
import './styles/themes/_base.css'
import './styles/themes/paper.css'
import './styles/themes/ink.css'
import './styles/extends.css'
import 'katex/dist/katex.min.css'
import './prose.css'
import i18n from './locales'
import { vTooltip } from './directives/tooltip'
import App from './App.vue'

markBoot('boot:modules')

// 全局拦截链接点击：外部 URL 用系统浏览器打开，阻止 webview 内导航
document.addEventListener('click', (e) => {
  const anchor = (e.target as HTMLElement).closest('a[href]') as HTMLAnchorElement | null
  if (!anchor) return
  const href = anchor.getAttribute('href')
  if (!href) return
  // 页内锚点、javascript: 等不拦截
  if (href.startsWith('#') || href.startsWith('javascript:')) return
  e.preventDefault()
  e.stopPropagation()
  invoke('plugin:shell|open', { path: href }).catch(() => {
    // shell 插件不可用时回退 window.open（dev 环境下）
    window.open(href, '_blank')
  })
})

// dragDropEnabled 已关闭（HTML5 拖放需要），webview 对拖入文件的默认行为是导航打开它；
// 全局吃掉默认行为，实际收图由各输入区自己的 drop 监听承接（preventDefault 不阻断冒泡）
window.addEventListener('dragover', (e) => e.preventDefault())
window.addEventListener('drop', (e) => {
  // 纯文本拖进 textarea/input 放行浏览器原生插入，只拦文件（防导航）
  const isTextTarget = e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement
  if (isTextTarget && !e.dataTransfer?.types.includes('Files')) return
  e.preventDefault()
})

createApp(App).use(i18n).directive('tooltip', vTooltip).mount('#app')
markBoot('boot:mounted')
finishBootTrace()

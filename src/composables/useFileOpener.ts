import { invoke } from '@tauri-apps/api/core'
import { Menu } from '@tauri-apps/api/menu'
import i18n from '@/locales'

type OpenAction = () => void | Promise<void>

/** 按 Monet 的统一扩展名策略打开本地路径。 */
export function openPath(path: string, systemDefault = false): Promise<void> {
  return invoke('open_path', { path, systemDefault })
}

/** 网页链接使用独立命令，避免与本地路径策略混淆。 */
export function openExternalUrl(url: string): Promise<void> {
  return invoke('open_external_url', { url })
}

/** 文件入口的原生右键菜单，提供系统关联与文件定位兜底。 */
export async function showSystemOpenMenu(
  event: MouseEvent,
  openWithSystemDefault: OpenAction,
  path?: string,
): Promise<void> {
  event.preventDefault()
  event.stopPropagation()

  const items: Array<{ text: string; action: () => void }> = [
    {
      text: i18n.global.t('common.openWithSystemDefault'),
      action: () => {
        void Promise.resolve(openWithSystemDefault()).catch(() => {})
      },
    },
  ]

  if (path) {
    items.push({
      text: i18n.global.t('common.revealInFileManager'),
      action: () => {
        void invoke('reveal_in_finder', { path }).catch(() => {})
      },
    })
  }

  const menu = await Menu.new({ items })
  await menu.popup()
}

/** 普通路径入口的右键菜单快捷封装。 */
export function showPathOpenMenu(event: MouseEvent, path: string): Promise<void> {
  return showSystemOpenMenu(event, () => openPath(path, true), path)
}

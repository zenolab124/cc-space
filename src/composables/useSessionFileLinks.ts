import type { InjectionKey, Ref } from 'vue'

/** Markdown 文件链接所属的会话工作目录；后端仍会再次校验真实路径边界。 */
export const SESSION_FILE_ROOT: InjectionKey<Readonly<Ref<string | null | undefined>>> =
  Symbol('session-file-root')

/** 遗态 Worktree 的主目录映射目标；仅在旧路径不可用时由 Rust 安全映射。 */
export const SESSION_FILE_FALLBACK_ROOT: InjectionKey<Readonly<Ref<string | null | undefined>>> =
  Symbol('session-file-fallback-root')

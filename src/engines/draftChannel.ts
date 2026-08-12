import type { EngineRunConfig } from './runConfig'
import type { ProjectRef, SessionRef } from './types'

export interface RuntimeEngineDraft {
  reference: SessionRef
  project: ProjectRef
  engineName: string
  cwd: string
  attachedChannel: string | null
}

export interface DraftChannelReplacement {
  sessionId: string
  reference: SessionRef
  runtimeId: unknown
  attachedChannel: string | null
}

function canonicalChannel(channelId: string | null): string | null {
  // `official` 与 null 对 Codex 都是不注入 Provider；比较运行时绑定时视为同一渠道。
  return channelId === 'official' ? null : channelId
}

export function sameRuntimeChannel(left: string | null, right: string | null): boolean {
  return canonicalChannel(left) === canonicalChannel(right)
}

interface RebindDraftChannelRequest {
  sessionId: string
  draft: RuntimeEngineDraft
  selectedChannel: string | null
  options: Record<string, unknown>
  config: EngineRunConfig | null
}

interface RebindDraftChannelDependencies {
  createSession: (
    project: ProjectRef,
    cwd: string,
    options: Record<string, unknown>,
  ) => Promise<{ session: SessionRef; runtimeId: unknown }>
  sessionId: (session: SessionRef) => string
  stageDraft: (sessionId: string, draft: RuntimeEngineDraft) => void
  saveConfig: (sessionId: string, config: EngineRunConfig) => void
  beforeReplace?: (replacement: DraftChannelReplacement) => void
  replaceSession: (sessionId: string, replacementSessionId: string) => boolean
  discardDraft: (sessionId: string) => void
  replacementError: () => Error
}

/**
 * thread/start 已绑定运行渠道。空线程在首条消息前切换渠道时，通过新建线程并
 * 原位替换草稿完成重绑定；任何一步失败都回滚新线程，原草稿继续可用。
 */
export async function rebindDraftChannel(
  request: RebindDraftChannelRequest,
  dependencies: RebindDraftChannelDependencies,
): Promise<DraftChannelReplacement | null> {
  if (sameRuntimeChannel(request.draft.attachedChannel, request.selectedChannel)) {
    return null
  }

  const created = await dependencies.createSession(
    request.draft.project,
    request.draft.cwd,
    request.options,
  )
  const replacementSessionId = dependencies.sessionId(created.session)
  const replacement = {
    sessionId: replacementSessionId,
    reference: created.session,
    runtimeId: created.runtimeId,
    attachedChannel: request.selectedChannel,
  }

  try {
    dependencies.stageDraft(replacementSessionId, {
      reference: created.session,
      project: request.draft.project,
      engineName: request.draft.engineName,
      cwd: request.draft.cwd,
      attachedChannel: request.selectedChannel,
    })
    if (request.config) dependencies.saveConfig(replacementSessionId, request.config)
    dependencies.beforeReplace?.(replacement)
    if (!dependencies.replaceSession(request.sessionId, replacementSessionId)) {
      throw dependencies.replacementError()
    }
  } catch (cause) {
    dependencies.discardDraft(replacementSessionId)
    throw cause
  }

  return replacement
}

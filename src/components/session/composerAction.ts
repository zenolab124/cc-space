export type ComposerAction = 'send' | 'stop'

export interface ComposerActionState {
  busy: boolean
  hasContent: boolean
  canSendWhileBusy: boolean
}

/**
 * 两套会话控制器共享同一套 Composer 行为：空闲时发送；运行中无输入时停止；
 * 运行中有输入且 adapter 能接收时仍显示发送。具体是排队还是注入当前 turn，
 * 由各自控制器与 runtime 实现决定。
 */
export function resolveComposerAction(state: ComposerActionState): ComposerAction {
  if (state.busy && (!state.hasContent || !state.canSendWhileBusy)) return 'stop'
  return 'send'
}

export function shouldSubmitComposer(event: KeyboardEvent): boolean {
  return event.key === 'Enter'
    && !event.shiftKey
    && !event.isComposing
    && event.keyCode !== 229
}

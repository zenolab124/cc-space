export function errorMessage(cause: unknown, fallback: string): string {
  if (typeof cause === 'string' && cause.trim()) return cause
  if (cause instanceof Error && cause.message.trim()) return cause.message
  if (cause && typeof cause === 'object' && 'message' in cause) {
    const message = (cause as { message?: unknown }).message
    if (typeof message === 'string' && message.trim()) return message
  }
  try {
    const serialized = JSON.stringify(cause)
    if (serialized && serialized !== '{}') return serialized
  } catch {
    // 循环引用等不可序列化异常回落到稳定文案。
  }
  return fallback
}

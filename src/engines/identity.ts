import type { EngineInstanceId, ProjectRef, SessionRef } from './types'

function base64Url(value: string): string {
  const bytes = new TextEncoder().encode(value)
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

export function instanceKey(instance: EngineInstanceId): string {
  return `ei1.${base64Url(instance.engineId)}.${base64Url(instance.instanceId)}`
}

export function projectKey(reference: ProjectRef): string {
  return `pr1.${base64Url(reference.engine.engineId)}.${base64Url(reference.engine.instanceId)}.${base64Url(reference.nativeId)}`
}

export function sessionKey(reference: SessionRef): string {
  return `sr1.${base64Url(reference.engine.engineId)}.${base64Url(reference.engine.instanceId)}.${base64Url(reference.nativeId)}`
}

export function sameInstance(left: EngineInstanceId, right: EngineInstanceId): boolean {
  return left.engineId === right.engineId && left.instanceId === right.instanceId
}

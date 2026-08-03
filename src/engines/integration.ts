import type { EngineDescriptor, EngineInstanceId, ProjectRef, SessionRef } from './types'
import { projectKey, sessionKey } from './identity'

interface UiIdentityAdapter {
  projectId(reference: ProjectRef): string
  sessionId(reference: SessionRef): string
}

const structuredIdentity: UiIdentityAdapter = {
  projectId: projectKey,
  sessionId: sessionKey,
}
const nativeIdentity: UiIdentityAdapter = {
  projectId: reference => reference.nativeId,
  sessionId: reference => reference.nativeId,
}
const integrations = new Map<string, EngineDescriptor['ui']>()

export function configureUiIntegrations(descriptors: EngineDescriptor[]) {
  integrations.clear()
  for (const descriptor of descriptors) {
    integrations.set(`${descriptor.instance.engineId}/${descriptor.instance.instanceId}`, descriptor.ui)
  }
}

function identityAdapter(instance: EngineInstanceId): UiIdentityAdapter {
  return integrations.get(`${instance.engineId}/${instance.instanceId}`)?.identity === 'native'
    ? nativeIdentity
    : structuredIdentity
}

export function projectUiId(reference: ProjectRef): string {
  return identityAdapter(reference.engine).projectId(reference)
}

export function sessionUiId(reference: SessionRef): string {
  return identityAdapter(reference.engine).sessionId(reference)
}

export function usesNativeSessionSurface(instance: EngineInstanceId): boolean {
  return integrations.get(`${instance.engineId}/${instance.instanceId}`)?.sessionSurface === 'native'
}

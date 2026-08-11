import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('macOS background service recovery', () => {
  it('treats a fresh notFound service as registerable', () => {
    const services = source('../../src-tauri/src/background_services.rs')

    expect(services).toMatch(
      /matches!\(\s*status,\s*ServiceStatus::NotRegistered \| ServiceStatus::NotFound\s*\)/,
    )
    expect(services).toContain('service_management::register(kind, value)')
    expect(services).not.toContain('is not present in the app bundle')
  })

  it('does not boot out a managed service when no legacy plist exists', () => {
    const management = source('../../src-tauri/src/service_management.rs')
    const cleanupStart = management.indexOf('pub fn remove_legacy_launch_agent(label: &str)')
    const cleanup = management.slice(
      cleanupStart,
      management.indexOf('#[cfg(not(target_os = "macos"))]', cleanupStart),
    )

    expect(cleanup).toContain('if !plist.exists()')
    expect(cleanup.indexOf('if !plist.exists()'))
      .toBeLessThan(cleanup.indexOf('.args(["bootout", &service])'))
  })

  it('re-registers an enabled service whose launchd task is missing or unhealthy', () => {
    const services = source('../../src-tauri/src/background_services.rs')
    const management = source('../../src-tauri/src/service_management.rs')

    expect(services).toContain('runtime_recovery_required')
    expect(services).toContain('service_management::launchd_service_healthy(launchd_label)')
    expect(services).toContain('service_management::unregister(kind, value)')
    expect(services).toContain('recovery registration failed')
    expect(management).toContain('pub fn launchd_service_healthy(label: &str)')
    expect(management).toContain('needs LWCR update')
    expect(management).toContain('job state = spawn failed')
    expect(management).toContain('EX_CONFIG')
  })

  it('re-registers only unhealthy routine agents after a runner signature change', () => {
    const scheduler = source('../../src-tauri/src/scheduler.rs')
    const needsUpdateStart = scheduler.indexOf(
      'pub fn needs_update(routine: &RoutineDefinition, runner_path: &Path) -> bool',
    )
    const needsUpdate = scheduler.slice(
      needsUpdateStart,
      scheduler.indexOf('pub fn cleanup_orphans', needsUpdateStart),
    )

    expect(needsUpdate).toContain(
      'crate::service_management::launchd_service_healthy(&label(&routine.id))',
    )
    expect(needsUpdate).toContain('existing.trim() != expected.trim()')
    expect(scheduler).toContain('routine agent outdated, reinstalling')
  })

  it('refreshes a missing or stale Widget snapshot before registering the scheduler', () => {
    const widget = source('../../src-tauri/src/widget.rs')
    const startup = widget.slice(
      widget.indexOf('pub fn startup_sync()'),
      widget.indexOf('pub fn ensure_launch_agent()'),
    )
    const entry = source('../../src-tauri/src/lib.rs')

    expect(widget).toContain('snapshot_needs_refresh()')
    expect(startup).toContain('refresh_snapshot_if_needed()')
    expect(startup).toContain('ensure_launch_agent()')
    expect(startup.indexOf('refresh_snapshot_if_needed()'))
      .toBeLessThan(startup.indexOf('ensure_launch_agent()'))
    expect(entry).toContain('spawn_blocking(widget::startup_sync)')
  })

  it('atomically replaces both Widget snapshot outputs', () => {
    const appWriter = source('../../src-tauri/src/widget.rs')
    const updaterWriter = source('../../src-tauri/src/bin/widget_updater.rs')

    for (const writer of [appWriter, updaterWriter]) {
      expect(writer).toContain('FileExt::lock_exclusive')
      expect(writer).toContain('SNAPSHOT_WRITE_SEQUENCE.fetch_add')
      expect(writer).toContain('rename(&temporary_path, path)')
      expect(writer).toContain('remove_file(&temporary_path)')
    }
  })

  it('keeps the sandbox contract limited to the Widget-owned container', () => {
    const mainEntitlements = source('../../src-tauri/Monet.entitlements')
    const widgetEntitlements = source('../../src-widget/MonetWidgetExtension.entitlements')

    expect(mainEntitlements).not.toContain('application-groups')
    expect(widgetEntitlements).toContain('com.apple.security.app-sandbox')
    expect(widgetEntitlements).not.toContain('temporary-exception')
  })
})

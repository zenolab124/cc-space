# Monet Engine Adapter Guide

[中文](ENGINE_ADAPTERS.zh-CN.md)

Monet's engine layer keeps coding-agent protocol differences inside adapters. A new engine should pay only for its own protocol, process, and mapping work; it must not force the Archive, search, Workbench, notifications, or storage to grow another platform branch.

## Core model

- `EngineInstanceId`: one concrete installation or data-source instance of an engine.
- `ProjectRef` / `SessionRef`: global identities composed from an engine instance and an opaque `nativeId`.
- `SessionSource`: projects, sessions, timelines, search documents, assets, session actions, and change delivery.
- `AgentRuntime`: create/resume sessions, start turns, accept input while a turn is running, interrupt turns, answer interactions, and close.
- `EngineCapabilities` / `SessionActions`: decide what the UI exposes; shared UI must never infer capabilities from an engine name.
- Facets: optional assets, automation, configuration, quota, runtime-command, and model-catalog providers.

The neutral timeline consists of `ConversationRecord` and `Segment`. An adapter maps provider objects into `text`, `reasoning`, `toolCall`, `toolResult`, `commandExecution`, `fileChange`, `attachment`, or a bounded `unknown`. Provider wire formats must not pass directly into the frontend.

## Adding an adapter

1. Implement the locator, protocol client/supervisor, source, runtime, and adapter under `src-tauri/src/engines/<engine>/`.
2. Create a stable `EngineInstanceId` for the default instance. Implement `EngineAdapter::descriptor`, `health`, and `session_source`; return a runtime or facets only when supported.
3. Statically register the descriptor and lazy constructor through `register_configured_adapter` in `src-tauri/src/engines/system.rs`. Production builds never register `FixtureEngine`.
4. Map every provider object into Core types. Generic commands and events already exist; do not add top-level `<engine>_*` IPC.
5. Add tests for the protocol schema, unknown fields, pagination, errors, identity isolation, and runtime events.

Minimal shape:

```rust
impl EngineAdapter for ExampleEngine {
    fn descriptor(&self) -> EngineDescriptor { self.descriptor.clone() }
    fn health(&self) -> EngineFuture<'_, EngineHealth> { /* ... */ }
    fn session_source(&self) -> &dyn SessionSource { &self.source }
    fn runtime(&self) -> Option<&dyn AgentRuntime> { Some(&self.runtime) }
}
```

## UI integration

New engines use structured UI identities and the standard session surface by default:

```rust
ui: EngineUiIntegration {
    identity: UiIdentityMode::Structured,
    session_surface: SessionSurface::Standard,
    install_guide_url: Some("https://example.com/install".into()),
    configuration_guide_url: Some("https://example.com/config".into()),
}
```

The standard surface consumes only descriptors, `SessionActions`, the neutral timeline, and unified runtime events. Use `SessionSurface::Native` only for a first-party engine with an existing specialized surface that cannot yet be replaced; it is not an escape hatch for adding engine-name checks to shared components.

`sendWhileRunning` only says whether the upper layer can keep accepting user input during an active turn; it does not prescribe delivery. An adapter may inject the input into the active turn or queue it for the next turn. Shared UI always presents this as “Send” and must not expose provider protocol terminology.

Engine activation belongs to Monet settings. A disabled adapter remains visible in the engine catalog, but it is not constructed and cannot subscribe to a source, start a watcher, poll, or launch a resident process; enabling it takes effect after an app restart. If an adapter receives changes from an external legacy watcher, route them through `EngineAdapter::notify_source_change` and the adapter's own `subscribe_changes` channel instead of emitting a top-level Tauri event from the watcher.

For new user-visible text, update only `src/locales/zh-CN.json` and `src/locales/en-US.json`. Engine brand names plus installation and native-configuration guides come from descriptors; capability names and generic states use i18n.

## Data and security constraints

- Treat original transcripts, rollouts, and databases as read-only. Titles, tags, stars, and soft-delete markers belong in Monet metadata.
- Spawn external processes through the relevant locator or enhanced PATH; never rely on a development shell's inherited PATH.
- Prefer local stdio protocols and do not open an extra network listener.
- Never auto-approve an unknown interaction. Logs must not contain credentials, complete environments, or unbounded payloads.
- Route assets through opaque `AssetRef` values and read them lazily; cap large payloads.
- Deliver `SourceChange` events so search shards can invalidate precisely.
- Sanitize paths and error text in exported health diagnostics. Descriptors contain only stable static facts; installation, authentication, version, and handshake state belong in `EngineHealth`.

## Acceptance checklist

- Equal native project/session IDs in two instances never collide in metadata, search, Workbench, Runner, notifications, or assets.
- A missing engine, malformed protocol, or exited process does not block an existing engine.
- Source cursors are stable; sessions without a cwd have a stable uncategorized project.
- Runtime generations and sequences are ordered; late events from an old generation are rejected and sequence gaps converge through snapshots.
- Streaming crosses IPC in batches; idle supervisors do not poll rapidly; retries are bounded and jittered.
- Unknown items degrade safely, and unbounded raw payloads never enter IPC.
- All `FixtureEngine` source/runtime contract tests pass.
- The adapter adds no top-level command/event, changes no metadata/search/workbench schema, and introduces no engine-name branch in shared components.

Common checks:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib 'engines::' --locked
pnpm build
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
```

If the engine publishes a generated formal schema, add an explicit opt-in installed-version smoke test and validate both the minimum supported version and the current version.

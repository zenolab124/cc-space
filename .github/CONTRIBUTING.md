# Contributing to Monet

Thanks for your interest in contributing! This document covers the basics for getting a development environment running and the conventions we follow.

## Development Setup

Prerequisites:

- [Node.js](https://nodejs.org/) 22+
- [pnpm](https://pnpm.io/) 9+
- [Rust](https://www.rust-lang.org/tools/install) (stable, 1.77+)
- macOS 11+ (the app is macOS-first; Windows support is in progress)

```bash
pnpm install
pnpm tauri dev    # run the full app (Vite + Rust backend)
pnpm dev          # frontend only, in a browser
```

Other useful commands:

```bash
pnpm build        # type-check + build frontend
pnpm test         # run vitest
cargo clippy      # lint Rust code (run inside src-tauri/)
```

## Project Layout

- `src/` — Vue 3 + TypeScript frontend (components, views, composables, locales)
- `src-tauri/` — Rust backend (file watching, JSONL parsing, process management, system APIs)
- `src-tray/`, `src-widget/` — macOS menu bar helper and WidgetKit widget

Frontend ↔ backend communication goes through Tauri commands (`invoke()`).

## Core Design Rules

These are hard constraints — PRs that violate them won't be merged:

1. **Never write to Claude Code's JSONL files.** Monet is strictly read-only over `~/.claude/projects/`. All value-added data (titles, tags, archive state, generated content) lives in separate files under `~/.monet/`.
2. **Never spawn external user-level tools by bare command name.** Packaged .app environments have a minimal `PATH`. Use `claude_locator` (for `claude`) or inject `streaming::enhanced_path()` (for others). Only `/usr/bin` and `/bin` system commands may be called bare.
3. **All user-facing UI text goes through vue-i18n.** New keys only need `zh-CN.json` and `en-US.json`; the other locales fall back automatically.

## Conventions

- One component per file, PascalCase filenames; consider splitting components over ~150 lines
- TypeScript strict mode; complete type definitions
- Rust code should pass default clippy rules
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `style:`, `refactor:`, `chore:`)
- **Never edit version fields by hand** (`package.json` / `tauri.conf.json` / `Cargo.toml` are kept in sync by a version hook; maintainers bump via `pnpm version`)

## Submitting Changes

1. Fork the repo and create a branch from `main`
2. Make your changes; keep PRs focused on a single topic
3. Make sure `pnpm build` and `pnpm test` pass
4. Open a PR describing what changed and why

For larger features, please open an issue first to discuss the direction before investing significant time.

## Reporting Bugs

Use the bug report issue template. Including your Monet version, macOS version, and Claude Code CLI version makes issues much easier to reproduce.

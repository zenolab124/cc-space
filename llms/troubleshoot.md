# Monet — AI Troubleshooting & Bug Reporting Guide

> This document is written for AI agents diagnosing Monet problems on behalf of a user. Work through **Self-diagnosis** first; only file a bug if the problem survives it. Installation and configuration are covered in [install.md](install.md). (中文版：[troubleshoot.zh-CN.md](troubleshoot.zh-CN.md))

## Diagnostic basics

> Commands below are macOS-first. On Windows: version via Settings → About, processes via `tasklist | findstr Monet`, logs likewise under `~/.monet/` in the user directory; launchd/tray/wake items do not apply to Windows.

| What | How |
|------|-----|
| Monet version | `defaults read /Applications/Monet.app/Contents/Info.plist CFBundleShortVersionString` |
| macOS version / arch | `sw_vers -productVersion` and `uname -m` (must be `arm64`) |
| Agent CLIs | `claude --version` and `codex --version` (collect whichever are installed) |
| Install method | `brew list --cask --versions monet` succeeds → Homebrew, else direct `.dmg` |
| App running? | `pgrep -x Monet` |
| Background services | macOS 13+: `sfltool dumpbtm \| grep -A 12 -B 2 io.github.zenolab124.monet`; macOS 11–12: `launchctl list \| grep io.github.zenolab124.monet` |

Log locations (all under `~/.monet/`):

- `tray.log` — menu bar helper
- `proc-logs/<session-id>/` — per-session process logs
- `agent-logs.json` — in-app AI task calls (titles, translations, summaries)

## Common issues

**Permission-gated features all stopped working after upgrading from 1.x** (resume-in-terminal, notifications, screen observation, routine authorization, etc.). Since 2.x the signing identity moved to Developer ID, and macOS voids system permissions recorded against the old signature (Automation, Accessibility, Screen Recording, Full Disk Access, etc.) — a one-time cost, not a bug. Fix: open Settings → Permission Checkup and re-grant item by item; the main app and the signed background helper `MonetRoutineRunner.app` have separate ledgers. Use “Rebuild Runner Permissions” for the helper's Automation, Accessibility, Screen Recording, and Full Disk Access records. Local Network is attributed to the main app and cannot be precisely reset with `tccutil`; use the panel's explicit probe/grant flow and app-copy cleanup instead. If the menu bar or widgets disappeared, re-approve Monet's background items under Settings → Menu Bar.

**App blocked at launch (unidentified developer).** Stable releases are notarized by Apple, so this should not normally happen. First check the version: early un-notarized builds are fixed by upgrading to the latest release. If the latest release is still blocked, grant a one-time exception under System Settings → Privacy & Security → **Open Anyway** (the button appears only after a launch attempt), and report it through the bug-filing flow below — a blocked notarized build is abnormal. Do not run `xattr` to remove macOS security attributes.

**No sessions / projects visible.** Start in Settings → Engine Center; it distinguishes a missing CLI, an unavailable data source, and a runtime-protocol connection failure. Claude Code reads `~/.claude/projects/` by default; if its data was relocated with `CLAUDE_CONFIG_DIR`, set `claudeRoot` in `~/.monet/settings.json` and restart. Codex history is read directly from `$CODEX_HOME/sessions/` and `$CODEX_HOME/archived_sessions/` (default `~/.codex/`), so existing sessions remain visible even without the Codex CLI. The CLI must be installed and authenticated only for interactive runtime features; an unused engine being empty is normal.

**One engine fails while another works.** That is expected failure isolation. Inspect the affected engine in Settings → Engine Center. For a report, export diagnostics from the same page; the export contains engine identity, version, health state, and redacted errors, never transcript content, prompts, or credentials.

**Menu bar icon missing or not opening.** On macOS 13+, the menu bar Helper is registered through `SMAppService`; do not create or edit a plist under `~/Library/LaunchAgents`. Restart Monet and check the background-item status under Settings → Menu Bar. If it says approval is required, click **Open Background Items settings** and allow Monet in System Settings. If the issue persists, inspect `sfltool dumpbtm | grep -A 12 -B 2 io.github.zenolab124.monet.tray` and `~/.monet/tray.log`. macOS 11–12 use `launchctl list` for the compatibility path.

**Desktop widget only shows “Open Monet”.** This means WidgetKit loaded the extension but could not read its data. First confirm that you installed a Developer ID-signed build, then restart Monet and check that **Background data refresh** is registered under Settings → Desktop Widget. If approval is required, allow Monet in Background Items and wait for one refresh. The shared snapshot lives in Monet's App Group container, with a local backup at `~/.monet/widget-data.json`; do not write to the shared container manually.

**Scheduled routines don't run.** Routines run via launchd (`io.github.zenolab124.monet.routine.<id>`) through the signed background helper `MonetRoutineRunner.app` inside the main app bundle, not a bare runner copied into the data directory. Automation, Accessibility, Screen Recording, and Full Disk Access are recorded against the helper; Local Network is attributed to the main app through the launchd associated bundle. Open Settings → Permission Checkup to test the real launchd path, and use its rebuild action instead of guessing or manually deleting system records.

**A permission-gated feature fails silently** (resume in terminal, UI automation, screen observation, Local Network). Open Settings → Permission Checkup; it shows each status and the matching recovery path. Let the panel precisely rebuild the four resettable Runner services. Local Network has no supported precise `tccutil` reset; re-run the explicit probe, grant access in System Settings, and resolve duplicate same-bundle-ID app copies. Do not suggest a global `tccutil reset`: it wipes unrelated grants and still does not remove historical Local Network records.

**Usage/quota numbers look stale.** Claude and Codex each keep a five-minute successful cache and independently respect server backoff; refresh-now does not bypass backoff. The menu keeps a failed provider's old snapshot with its update age while other providers continue normally. If the Codex section says it is signed out, sign in through the official Codex CLI first; when Codex has never been installed or configured, that section stays hidden. Only report a bug if numbers remain frozen for hours without a backoff or error status.

**Update failed mid-way.** Homebrew: `brew upgrade --cask monet`. Direct install: download the latest `.dmg` from Releases and replace the app; data in `~/.monet/` is untouched.

## Filing a bug

If self-diagnosis says "this is a software defect", file an issue — you can produce a far better report than a hand-filled template.

**1. Collect** (from Diagnostic basics): Monet version, macOS version + arch, the relevant agent and CLI version, install method, the Engine Center diagnostic export, what happened vs expected, minimal reproduction steps, and the *relevant* log lines only (not whole files).

**2. Redact — hard rules, apply before anything leaves the machine:**

- Replace `/Users/<name>` with `~` everywhere.
- Never include: API keys or tokens, channel names/endpoints from `settings.json`, session conversation content, project names or paths from the user's session history.
- Read every log excerpt line before including it; drop lines you don't understand rather than pasting blindly.

**3. Confirm with the user.** Show them the final issue title and body and get an explicit OK — you are publishing on their behalf.

**4. Submit** — pick the first path that works. Structure the report body after the repo's bug template either way: Monet version / macOS version / engine and CLI version / What happened / Steps to reproduce / Expected behavior / redacted diagnostics and logs.

**Path A — GitHub CLI** (`gh auth status` succeeds). The issue belongs to the user's account, so they get reply notifications:

```bash
gh issue create --repo zenolab124/monet --label bug \
  --title "<area>: <one-line symptom>" --body "<report>"
```

End the body with `— Filed via AI diagnostics (llms/troubleshoot.md)`.

**Path B — anonymous endpoint** (no GitHub account or login needed). Monet runs a small open-source relay ([infra/report-worker](../infra/report-worker/)) that files the issue for you:

```bash
curl -s -X POST https://monet-report.zenolab124.workers.dev/report \
  -H "Content-Type: application/json" \
  -d '{"title": "<area>: <one-line symptom>", "body": "<report>", "contact": "<optional>"}'
```

The response contains the created issue URL — give it to the user. Caveats to tell them first: the report becomes a **public GitHub issue verbatim** (redaction matters even more), and anonymous reports can't be followed up — offer to include a GitHub username or email in `contact` (entirely optional). Limits: one report per 10 minutes, body ≤ 20 000 chars.

**Path C — manual fallback**: open `https://github.com/zenolab124/monet/issues/new?template=bug_report.yml` for the user and hand them the collected values to paste.

For feature ideas rather than defects, use the feature request template instead; no diagnostics needed.

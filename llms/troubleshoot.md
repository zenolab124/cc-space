# Monet — AI Troubleshooting & Bug Reporting Guide

> This document is written for AI agents diagnosing Monet problems on behalf of a user. Work through **Self-diagnosis** first; only file a bug if the problem survives it. Installation and configuration are covered in [install.md](install.md). (中文版：[troubleshoot.zh-CN.md](troubleshoot.zh-CN.md))

## Diagnostic basics

| What | How |
|------|-----|
| Monet version | `defaults read /Applications/Monet.app/Contents/Info.plist CFBundleShortVersionString` |
| macOS version / arch | `sw_vers -productVersion` and `uname -m` (must be `arm64`) |
| Claude Code CLI | `claude --version` |
| Install method | `brew list --cask --versions monet` succeeds → Homebrew, else direct `.dmg` |
| App running? | `pgrep -x Monet` |
| Background services | `launchctl list \| grep io.github.zenolab124.monet` |

Log locations (all under `~/.monet/`):

- `tray.log` — menu bar helper
- `proc-logs/<session-id>/` — per-session process logs
- `agent-logs.json` — in-app AI task calls (titles, translations, summaries)

## Common issues

**App blocked at launch ("damaged" / unidentified developer).** Gatekeeper reaction to a signed-but-not-notarized app. Fix: `xattr -cr /Applications/Monet.app`, then reopen. One-time; in-app updates are silent afterwards.

**No sessions / projects visible.** Monet reads `~/.claude/projects/` by default. Check the directory exists and has content. If the user relocated Claude Code data (`CLAUDE_CONFIG_DIR`), set `claudeRoot` in `~/.monet/settings.json` (see install.md) and restart. An empty archive on a machine that never ran Claude Code is normal.

**Menu bar icon missing.** Check `launchctl list | grep io.github.zenolab124.monet.tray`. Restart the app (the tray is re-registered on launch). Inspect `~/.monet/tray.log` for errors.

**Scheduled routines don't run.** Routines run via launchd (`io.github.zenolab124.monet.routine.<id>`) with a permission ledger **separate from the main app** — the executable is `monet-routine-runner`. First-time grants happen via system prompts during an actual run; if a prompt was denied, macOS won't re-ask — the user must remove the old `monet-routine-runner` entry in System Settings → Privacy & Security, then re-trigger. Settings has a permission health-check panel that tests the real launchd path.

**A permission-gated feature fails silently** (resume in terminal, UI automation, screen observation). Open Settings → permission health check; it shows exactly what's granted and how to fix each. Do not suggest `tccutil reset` unless the panel's guidance fails — it wipes grants app-wide.

**Usage/quota numbers look stale.** The usage API rate-limits aggressively; Monet backs off (~15 min) rather than hammering it. Waiting is the fix. Only report a bug if numbers stay frozen for hours.

**Update failed mid-way.** Homebrew: `brew upgrade --cask monet`. Direct install: download the latest `.dmg` from Releases and replace the app; data in `~/.monet/` is untouched.

## Filing a bug

If self-diagnosis says "this is a software defect", file an issue — you can produce a far better report than a hand-filled template.

**1. Collect** (from Diagnostic basics): Monet version, macOS version + arch, CLI version, install method, what happened vs expected, minimal reproduction steps, and the *relevant* log lines only (not whole files).

**2. Redact — hard rules, apply before anything leaves the machine:**

- Replace `/Users/<name>` with `~` everywhere.
- Never include: API keys or tokens, channel names/endpoints from `settings.json`, session conversation content, project names or paths from the user's session history.
- Read every log excerpt line before including it; drop lines you don't understand rather than pasting blindly.

**3. Confirm with the user.** Show them the final issue title and body and get an explicit OK — you are publishing on their behalf.

**4. Submit** — pick the first path that works. Structure the report body after the repo's bug template either way: Monet version / macOS version / CLI version / What happened / Steps to reproduce / Expected behavior / Logs.

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

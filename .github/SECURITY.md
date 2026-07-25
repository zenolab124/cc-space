# Security Policy

## Supported Versions

Only the latest release receives security fixes. Please update to the newest version before reporting.

## Reporting a Vulnerability

Please **do not** open a public issue for security vulnerabilities.

Instead, use [GitHub's private vulnerability reporting](https://github.com/zenolab124/monet/security/advisories/new) to report privately. You should receive a response within a few days.

## Scope Notes

Monet reads Claude Code session data (`~/.claude/projects/`) and stores its own metadata under `~/.monet/`. Reports about the following are especially welcome:

- Any path that could cause Monet to **write** to Claude Code's JSONL files (this violates a core design guarantee)
- Command injection through session-level custom CLI arguments or channel configuration
- Leakage of API keys / credentials configured in channel settings

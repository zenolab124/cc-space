# monet-report worker

Anonymous bug-report endpoint for AI agents without a GitHub login. See the
"Filing a bug" section of [llms-troubleshoot.md](../../llms-troubleshoot.md)
for the client-side protocol, and `src/index.js` for exactly what the server
does with a submission (files a public GitHub issue, nothing else).

## API

Production: `https://monet-report.zenolab124.workers.dev`

```
POST /report
Content-Type: application/json

{ "title": "...", "body": "...", "contact": "optional" }
→ { "ok": true, "url": "https://github.com/zenolab124/monet/issues/N" }
```

Limits: title ≤ 200 chars, body ≤ 20 000 chars, one report per IP per
10 minutes. Submissions become **public** GitHub issues verbatim — redact
before sending.

## Deploy (maintainers)

```bash
cd infra/report-worker
wrangler deploy
wrangler secret put GITHUB_TOKEN   # fine-grained PAT: this repo only, Issues read/write
```

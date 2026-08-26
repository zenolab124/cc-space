# monet-report worker

Public Issue relay for clients without a GitHub login. See the
"Filing a bug" section of [llms/troubleshoot.md](../../llms/troubleshoot.md)
for the client-side protocol, and `src/index.js` for exactly what the server
does with a submission (files a public GitHub issue, nothing else). The same
Issues-only token also accepts strict declarative theme submissions; it never
receives repository content or pull-request permissions.

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

Theme submissions use `POST /theme-submission` with
`{ "theme": <schema-v1-theme>, "publicName": "optional" }`. The Worker repeats
the field allowlist, range, unsafe-content, size, and WCAG AA checks before
creating an Issue labeled `theme-submission`; the cooldown is 30 minutes.

## Deploy (maintainers)

```bash
cd infra/report-worker
wrangler deploy
wrangler secret put GITHUB_TOKEN   # fine-grained PAT: this repo only, Issues read/write
```

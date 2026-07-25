/**
 * Monet anonymous bug-report endpoint (Cloudflare Worker).
 *
 * Accepts POST /report from AI agents that have no GitHub login (see
 * llms/troubleshoot.md), and files a GitHub issue on the Monet repo using a
 * server-held fine-grained PAT (Issues read/write on this repo only).
 *
 * Abuse controls: honeypot field, size caps, per-IP cooldown via the Cache
 * API (per-colo, not globally strict — good enough since one client IP
 * normally hits one colo). If abuse ever outgrows this, switch to a
 * review-queue mode instead of filing issues directly.
 */

const MAX_TITLE = 200
const MAX_BODY = 20_000
const COOLDOWN_SECONDS = 600

const json = (data, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'content-type': 'application/json' },
  })

export default {
  async fetch(req, env, ctx) {
    const url = new URL(req.url)
    if (req.method !== 'POST' || url.pathname !== '/report') {
      return json({ ok: false, error: 'usage: POST /report {title, body, contact?}' }, 404)
    }

    let payload
    try {
      payload = await req.json()
    } catch {
      return json({ ok: false, error: 'invalid JSON' }, 400)
    }

    // Honeypot: real clients never send this field; bots that do get a fake success.
    if (payload.website) return json({ ok: true, url: '' })

    const title = String(payload.title ?? '').trim()
    const body = String(payload.body ?? '').trim()
    const contact = String(payload.contact ?? '').trim()
    if (!title || !body) return json({ ok: false, error: 'title and body are required' }, 422)
    if (title.length > MAX_TITLE || body.length > MAX_BODY) {
      return json({ ok: false, error: `limits: title ${MAX_TITLE} chars, body ${MAX_BODY} chars` }, 413)
    }

    const ip = req.headers.get('cf-connecting-ip') ?? 'unknown'
    const cooldownKey = new Request(`https://cooldown.invalid/${ip}`)
    const cache = caches.default
    if (await cache.match(cooldownKey)) {
      return json({ ok: false, error: 'rate limited — one report per 10 minutes' }, 429)
    }

    const footer = [
      '',
      '---',
      contact && `Contact (voluntary): ${contact}`,
      '— Filed anonymously via AI diagnostics ([llms/troubleshoot.md](../blob/main/llms/troubleshoot.md))',
    ].filter(Boolean).join('\n')

    const gh = await fetch(`https://api.github.com/repos/${env.REPO}/issues`, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${env.GITHUB_TOKEN}`,
        accept: 'application/vnd.github+json',
        'user-agent': 'monet-report-worker',
        'x-github-api-version': '2022-11-28',
      },
      body: JSON.stringify({ title, body: body + '\n' + footer, labels: ['bug', 'ai-report'] }),
    })
    if (!gh.ok) {
      // Don't leak upstream details to anonymous callers.
      return json({ ok: false, error: `upstream error ${gh.status}` }, 502)
    }
    const issue = await gh.json()

    ctx.waitUntil(
      cache.put(
        cooldownKey,
        new Response('1', { headers: { 'cache-control': `max-age=${COOLDOWN_SECONDS}` } }),
      ),
    )
    return json({ ok: true, url: issue.html_url })
  },
}

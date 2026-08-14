export const ARTIFACT_SIZE_MESSAGE = 'monet:artifact-size'
export const MIN_ARTIFACT_FRAME_HEIGHT = 240

export function clampArtifactFrameHeight(contentHeight: number, frameWidth: number): number {
  const portraitFourThreeHeight = Math.max(1, frameWidth) * 4 / 3
  return Math.floor(Math.min(
    portraitFourThreeHeight,
    Math.max(MIN_ARTIFACT_FRAME_HEIGHT, contentHeight),
  ))
}

function measurementScript(token: string): string {
  return `(() => {
    const type = ${JSON.stringify(ARTIFACT_SIZE_MESSAGE)};
    const token = ${JSON.stringify(token)};
    let timer = 0;
    const measure = () => {
      timer = 0;
      const body = document.body;
      const root = document.documentElement;
      const height = Math.ceil(Math.max(
        root.scrollHeight,
        root.getBoundingClientRect().height,
        body ? body.scrollHeight : 0,
        body ? body.getBoundingClientRect().height : 0,
      ));
      parent.postMessage({ type, token, height }, '*');
    };
    const schedule = () => {
      if (!timer) timer = setTimeout(() => requestAnimationFrame(measure), 100);
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(document.documentElement);
    if (document.body) observer.observe(document.body);
    addEventListener('load', schedule, { once: true });
    schedule();
  })();`
}

export function prepareSandboxedHtml(source: string, nonce: string): string {
  const document = new DOMParser().parseFromString(source, 'text/html')

  document.querySelectorAll('script').forEach(element => element.remove())
  document.querySelectorAll('*').forEach(element => {
    for (const attribute of [...element.attributes]) {
      if (attribute.name.toLowerCase().startsWith('on')) element.removeAttribute(attribute.name)
    }
  })
  document.querySelectorAll('meta[http-equiv]').forEach(element => {
    const directive = element.getAttribute('http-equiv')?.toLowerCase()
    if (directive === 'refresh' || directive === 'content-security-policy') element.remove()
  })
  document.querySelectorAll('a, area').forEach(element => {
    element.removeAttribute('href')
    element.removeAttribute('xlink:href')
  })
  document.querySelectorAll('base').forEach(element => element.remove())

  const policy = document.createElement('meta')
  policy.httpEquiv = 'Content-Security-Policy'
  policy.content = [
    "default-src 'none'",
    "img-src data: blob:",
    "media-src data: blob:",
    "font-src data:",
    "style-src 'unsafe-inline'",
    `script-src 'nonce-${nonce}'`,
    "connect-src 'none'",
    "frame-src 'none'",
    "child-src 'none'",
    "worker-src 'none'",
    "object-src 'none'",
    "form-action 'none'",
    "base-uri 'none'",
  ].join('; ')
  document.head.prepend(policy)

  const script = document.createElement('script')
  script.nonce = nonce
  script.textContent = measurementScript(nonce)
  document.body.append(script)

  return `<!doctype html>\n${document.documentElement.outerHTML}`
}

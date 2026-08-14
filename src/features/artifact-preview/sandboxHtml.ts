export const ARTIFACT_SIZE_MESSAGE = 'monet:artifact-size'
export const ARTIFACT_WHEEL_BOUNDARY_MESSAGE = 'monet:artifact-wheel-boundary'
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
    const wheelType = ${JSON.stringify(ARTIFACT_WHEEL_BOUNDARY_MESSAGE)};
    const token = ${JSON.stringify(token)};
    let timer = 0;
    const measure = () => {
      timer = 0;
      const body = document.body;
      const viewportHeight = innerHeight;
      let contentTop = Number.POSITIVE_INFINITY;
      let contentBottom = Number.NEGATIVE_INFINITY;
      const includeRect = rect => {
        if (rect.width <= 0 && rect.height <= 0) return;
        contentTop = Math.min(contentTop, rect.top);
        contentBottom = Math.max(contentBottom, rect.bottom);
      };
      if (body) {
        for (const element of body.children) {
          if (element.tagName === 'SCRIPT') continue;
          includeRect(element.getBoundingClientRect());
        }
        const range = document.createRange();
        range.selectNodeContents(body);
        includeRect(range.getBoundingClientRect());
        range.detach();
      }
      const contentHeight = Number.isFinite(contentTop)
        ? contentBottom - contentTop
        : 0;
      const bodyStyle = body ? getComputedStyle(body) : null;
      const bodySpacing = bodyStyle
        ? ['marginTop', 'marginBottom', 'paddingTop', 'paddingBottom']
            .reduce((sum, property) => sum + (parseFloat(bodyStyle[property]) || 0), 0)
        : 0;
      const height = Math.ceil(contentHeight + bodySpacing);
      const fillsViewport = height >= viewportHeight - 2;
      parent.postMessage({ type, token, height, fillsViewport }, '*');
    };
    const schedule = () => {
      if (!timer) timer = setTimeout(() => requestAnimationFrame(measure), 100);
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(document.documentElement);
    if (document.body) observer.observe(document.body);
    addEventListener('load', schedule, { once: true });
    schedule();

    let wheelAxis = null;
    let wheelIdleTimer = 0;
    const canScroll = (element, axis, delta) => {
      if (!(element instanceof Element)) return false;
      const style = getComputedStyle(element);
      const overflow = axis === 'x' ? style.overflowX : style.overflowY;
      if (overflow !== 'auto' && overflow !== 'scroll') return false;
      const position = axis === 'x' ? element.scrollLeft : element.scrollTop;
      const extent = axis === 'x'
        ? element.scrollWidth - element.clientWidth
        : element.scrollHeight - element.clientHeight;
      if (extent <= 1) return false;
      return delta < 0 ? position > 1 : position < extent - 1;
    };
    const hasScrollableTarget = (target, axis, delta) => {
      for (let element = target instanceof Element ? target : null; element; element = element.parentElement) {
        if (canScroll(element, axis, delta)) return true;
      }
      const root = document.scrollingElement;
      return root ? canScroll(root, axis, delta) : false;
    };
    addEventListener('wheel', event => {
      if (event.ctrlKey) return;
      const deltaX = event.deltaX || (event.shiftKey ? event.deltaY : 0);
      const deltaY = event.shiftKey && !event.deltaX ? 0 : event.deltaY;
      if (!deltaX && !deltaY) return;
      if (!wheelAxis) wheelAxis = Math.abs(deltaX) > Math.abs(deltaY) ? 'x' : 'y';
      if (wheelIdleTimer) clearTimeout(wheelIdleTimer);
      wheelIdleTimer = setTimeout(() => { wheelAxis = null; }, 160);
      const delta = wheelAxis === 'x' ? deltaX : deltaY;
      if (!delta || hasScrollableTarget(event.target, wheelAxis, delta)) return;
      event.preventDefault();
      parent.postMessage({
        type: wheelType,
        token,
        axis: wheelAxis,
        deltaX,
        deltaY,
        deltaMode: event.deltaMode,
      }, '*');
    }, { passive: false });
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

  const scrollPolicy = document.createElement('style')
  scrollPolicy.textContent = 'html { overflow-y: auto !important; } body { overflow-y: visible !important; }'
  document.head.append(scrollPolicy)

  const script = document.createElement('script')
  script.nonce = nonce
  script.textContent = measurementScript(nonce)
  document.body.append(script)

  return `<!doctype html>\n${document.documentElement.outerHTML}`
}

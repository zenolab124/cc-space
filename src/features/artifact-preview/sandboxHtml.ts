export const ARTIFACT_SIZE_MESSAGE = 'monet:artifact-size'
export const ARTIFACT_WHEEL_BOUNDARY_MESSAGE = 'monet:artifact-wheel-boundary'
export const ARTIFACT_RUNTIME_BLOCKED_MESSAGE = 'monet:artifact-runtime-blocked'
export const MIN_ARTIFACT_FRAME_HEIGHT = 240

interface SandboxedHtmlOptions {
  scriptNonce: string
  messageToken: string
  allowArtifactScripts: boolean
}

const ARTIFACT_SCRIPT_MARKER = 'data-monet-artifact-script'
const ARTIFACT_EVENT_STORE = 'data-monet-artifact-events'

export function clampArtifactFrameHeight(contentHeight: number, frameWidth: number): number {
  const portraitFourThreeHeight = Math.max(1, frameWidth) * 4 / 3
  return Math.floor(Math.min(
    portraitFourThreeHeight,
    Math.max(MIN_ARTIFACT_FRAME_HEIGHT, contentHeight),
  ))
}

function sandboxRuntimeScript(options: SandboxedHtmlOptions): string {
  const { scriptNonce, messageToken, allowArtifactScripts } = options
  return `(() => {
    const type = ${JSON.stringify(ARTIFACT_SIZE_MESSAGE)};
    const wheelType = ${JSON.stringify(ARTIFACT_WHEEL_BOUNDARY_MESSAGE)};
    const blockedType = ${JSON.stringify(ARTIFACT_RUNTIME_BLOCKED_MESSAGE)};
    const token = ${JSON.stringify(messageToken)};
    const allowArtifactScripts = ${JSON.stringify(allowArtifactScripts)};
    const scriptMarker = ${JSON.stringify(ARTIFACT_SCRIPT_MARKER)};
    const eventStore = ${JSON.stringify(ARTIFACT_EVENT_STORE)};
    const scriptNonce = ${JSON.stringify(scriptNonce)};
    document.currentScript?.remove();

    const hostBridgeExposed = Boolean(window.__TAURI_INTERNALS__ || window.isTauri);
    if (allowArtifactScripts && hostBridgeExposed) {
      parent.postMessage({ type: blockedType, token }, '*');
    } else if (allowArtifactScripts) {
      let eventBindingIndex = 0;
      const installEventHandler = (element, name, value) => {
        const eventName = name.slice(2);
        if (!eventName) return;
        const bindingKey = '__monetArtifactEvent_' + scriptNonce + '_' + eventBindingIndex++;
        window[bindingKey] = element;
        const handler = document.createElement('script');
        handler.setAttribute('nonce', scriptNonce);
        handler.textContent =
          'window[' + JSON.stringify(bindingKey) + '].addEventListener(' +
          JSON.stringify(eventName) +
          ', function(event) { const result = (function(event) { ' + value +
          '\\n }).call(this, event); if (result === false) event.preventDefault(); });';
        document.head.append(handler);
        handler.remove();
        delete window[bindingKey];
      };
      for (const element of document.querySelectorAll('[' + eventStore + ']')) {
        const serialized = element.getAttribute(eventStore);
        element.removeAttribute(eventStore);
        if (!serialized) continue;
        try {
          for (const [name, value] of JSON.parse(serialized)) installEventHandler(element, name, value);
        } catch {}
      }
      const placeholders = [...document.querySelectorAll('template[' + scriptMarker + ']')];
      for (const placeholder of placeholders) {
        const source = placeholder.content.querySelector('script');
        if (!source || source.hasAttribute('src')) {
          placeholder.remove();
          continue;
        }
        const active = document.createElement('script');
        for (const attribute of source.attributes) {
          if (attribute.name !== 'nonce' && attribute.name !== 'src') {
            active.setAttribute(attribute.name, attribute.value);
          }
        }
        active.setAttribute('nonce', scriptNonce);
        active.textContent = source.textContent;
        placeholder.replaceWith(active);
      }
    }

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

function neutralizeArtifactScripts(document: Document, allowArtifactScripts: boolean) {
  document.querySelectorAll(`[${ARTIFACT_SCRIPT_MARKER}], [${ARTIFACT_EVENT_STORE}]`).forEach(element => {
    element.removeAttribute(ARTIFACT_SCRIPT_MARKER)
    element.removeAttribute(ARTIFACT_EVENT_STORE)
  })

  document.querySelectorAll('script').forEach(element => {
    if (!allowArtifactScripts) {
      element.remove()
      return
    }
    const placeholder = document.createElement('template')
    placeholder.setAttribute(ARTIFACT_SCRIPT_MARKER, '')
    placeholder.content.append(element.cloneNode(true))
    element.replaceWith(placeholder)
  })

  document.querySelectorAll('*').forEach(element => {
    const eventAttributes = [...element.attributes]
      .filter(attribute => attribute.name.toLowerCase().startsWith('on'))
      .map(attribute => [attribute.name, attribute.value])
    for (const [name] of eventAttributes) element.removeAttribute(name)
    if (allowArtifactScripts && eventAttributes.length > 0) {
      element.setAttribute(ARTIFACT_EVENT_STORE, JSON.stringify(eventAttributes))
    }
  })
}

export function prepareSandboxedHtml(source: string, options: SandboxedHtmlOptions): string {
  const document = new DOMParser().parseFromString(source, 'text/html')

  neutralizeArtifactScripts(document, options.allowArtifactScripts)
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
    `script-src 'nonce-${options.scriptNonce}'`,
    "script-src-attr 'none'",
    "connect-src 'none'",
    "frame-src 'none'",
    "child-src 'none'",
    "worker-src 'none'",
    "object-src 'none'",
    "form-action 'none'",
    "base-uri 'none'",
    "navigate-to 'none'",
  ].join('; ')
  document.head.prepend(policy)

  const scrollPolicy = document.createElement('style')
  scrollPolicy.textContent = 'html { overflow-y: auto !important; } body { overflow-y: visible !important; }'
  document.head.append(scrollPolicy)

  const script = document.createElement('script')
  script.setAttribute('nonce', options.scriptNonce)
  script.textContent = sandboxRuntimeScript(options)
  document.body.append(script)

  return `<!doctype html>\n${document.documentElement.outerHTML}`
}

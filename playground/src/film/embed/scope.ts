/**
 * The film's stand-ins for page globals. The library build rewrites product and playground source to use these,
 * so the film reads its own palette, viewport, storage and fetch mocks, and never touches the host page.
 */

class MemoryStorage implements Storage {
  #items = new Map<string, string>();
  get length() { return this.#items.size; }
  clear() { this.#items.clear(); }
  getItem(key: string) { return this.#items.get(key) ?? null; }
  key(index: number) { return [...this.#items.keys()][index] ?? null; }
  removeItem(key: string) { this.#items.delete(key); }
  setItem(key: string, value: string) { this.#items.set(key, String(value)); }
}

const detached = document.createElement("div");
const roots: HTMLElement[] = [];
const nativeFetch: typeof fetch = (input, init) => fetch(input, init);
const stage = { width: 1920, height: 1080 };

const FEATURE = /^(not all and )?\(\s*(max-width|min-width|max-height|min-height|hover|pointer|any-hover|any-pointer)\s*:\s*([\w.]+?)(px)?\s*\)$/;

/** Answers one media query as the desktop app on a stage-sized screen would; null when it needs the real page. */
export function stageMatches(query: string, size: { width: number; height: number }): boolean | null {
  let any = false;
  for (const part of query.split(",").map((q) => q.trim())) {
    const m = part.match(FEATURE);
    if (!m) return null;
    const [, negate, feature, value] = m;
    let hit: boolean;
    if (feature.endsWith("hover")) hit = value === "hover";
    else if (feature.endsWith("pointer")) hit = value === "fine";
    else {
      const limit = Number(value);
      const actual = feature.endsWith("width") ? size.width : size.height;
      hit = feature.startsWith("max") ? actual <= limit : actual >= limit;
    }
    any ||= negate ? !hit : hit;
  }
  return any;
}

function staticList(media: string, matches: boolean): MediaQueryList {
  const noop = () => undefined;
  return { media, matches, onchange: null, addListener: noop, removeListener: noop, addEventListener: noop, removeEventListener: noop, dispatchEvent: () => false };
}

type Listener = EventListenerOrEventListenerObject;
const documentListeners = new WeakMap<Listener, Map<string, EventListener>>();

/** The element an event really started on when it started inside a film's shadow root, which the document sees as the host. */
function filmTarget(event: Event): EventTarget | null {
  const inner = event.composedPath()[0];
  if (!(inner instanceof Node) || inner === event.target) return null;
  const root = inner.getRootNode();
  return root instanceof ShadowRoot && root.host.classList.contains("domios-film") ? inner : null;
}

/** The same event, with `target` pointing inside the film, as the product's own document listeners see it un-embedded. */
function retarget(event: Event, target: EventTarget): Event {
  return new Proxy(event, {
    get(source, key) {
      if (key === "target") return target;
      const value = Reflect.get(source, key, source);
      return typeof value === "function" ? value.bind(source) : value;
    },
  });
}

const listenerKey = (type: string, options?: boolean | EventListenerOptions) => `${type}|${typeof options === "boolean" ? options : !!options?.capture}`;

export const __film = {
  /** Innermost mounted film root; a detached element before any mount so import-time writes go nowhere. */
  root: (): HTMLElement => roots[roots.length - 1] ?? detached,
  // No Tauri stand-in: on a real domain the daemon bootstrap would read it as a native app and demand a loopback origin.
  globals: {} as Record<string, unknown>,
  fetch: nativeFetch,
  localStorage: new MemoryStorage() as Storage,
  sessionStorage: new MemoryStorage() as Storage,
  get innerWidth() { return stage.width; },
  get innerHeight() { return stage.height; },
  visualViewport: null as VisualViewport | null,
  matchMedia: (query: string): MediaQueryList => {
    const matches = stageMatches(query, stage);
    return matches === null ? window.matchMedia(query) : staticList(query, matches);
  },
  // Outside-click and key handlers listen on the document; inside a shadow root they would see the host as the target
  // and dismiss a menu before its own item is clicked.
  addDocumentListener(type: string, listener: Listener | null, options?: boolean | AddEventListenerOptions) {
    if (!listener) return;
    let byKey = documentListeners.get(listener);
    if (!byKey) documentListeners.set(listener, (byKey = new Map()));
    const key = listenerKey(type, options);
    let wrapped = byKey.get(key);
    if (!wrapped) {
      wrapped = (event: Event) => {
        const target = filmTarget(event);
        const seen = target ? retarget(event, target) : event;
        if (typeof listener === "function") listener.call(document, seen);
        else listener.handleEvent(seen);
      };
      byKey.set(key, wrapped);
    }
    document.addEventListener(type, wrapped, options);
  },
  removeDocumentListener(type: string, listener: Listener | null, options?: boolean | EventListenerOptions) {
    const wrapped = listener && documentListeners.get(listener)?.get(listenerKey(type, options));
    if (wrapped) document.removeEventListener(type, wrapped, options);
  },
};

export function setStage(width: number, height: number) {
  stage.width = width;
  stage.height = height;
}

export function enterScope(root: HTMLElement): () => void {
  roots.push(root);
  return () => {
    const i = roots.lastIndexOf(root);
    if (i >= 0) roots.splice(i, 1);
  };
}

type Args = Record<string, unknown> | undefined;
type Handler = (args: Args) => unknown;
type Listener = (event: { event: string; id: number; payload: unknown }) => void;

const handlers = new Map<string, Handler>();
const lists = new Map<string, unknown[]>();
const listeners = new Map<string, Set<Listener>>();
const unhandled = new Set<string>();

export function mockCommand(command: string, handler: Handler): void {
  handlers.set(command, handler);
}

/** Every specimen appends its own rows; the command answers with all of them. */
export function provideList<T>(command: string, items: readonly T[]): void {
  const current = lists.get(command) ?? [];
  current.push(...items);
  lists.set(command, current);
}

const byTab = new Map<string, Map<string, unknown>>();

/** Answers `command` with `value` when it is called for `tabId`. */
export function provideForTab(command: string, tabId: string, value: unknown): void {
  const table = byTab.get(command) ?? new Map<string, unknown>();
  table.set(tabId, value);
  byTab.set(command, table);
}

const byWorkspace = new Map<string, Map<string, unknown>>();

/** Answers `command` with `value` when it is called for `workspaceId`, ahead of any shared handler or list. */
export function provideForWorkspace(command: string, workspaceId: string, value: unknown): void {
  const table = byWorkspace.get(command) ?? new Map<string, unknown>();
  table.set(workspaceId, value);
  byWorkspace.set(command, table);
}

export async function mockInvoke<T>(command: string, args?: Args): Promise<T> {
  const workspaceId = args?.workspaceId ?? args?.workspace_id;
  const scoped = byWorkspace.get(command);
  if (scoped && workspaceId != null && scoped.has(String(workspaceId))) return scoped.get(String(workspaceId)) as T;
  const handler = handlers.get(command);
  if (handler) return (await handler(args)) as T;
  const table = byTab.get(command);
  if (table) {
    const tabId = String(args?.tabId ?? args?.tab_id ?? "");
    return (table.has(tabId) ? table.get(tabId) : null) as T;
  }
  const list = lists.get(command);
  if (list) return [...list] as T;
  if (!unhandled.has(command)) {
    unhandled.add(command);
    console.debug(`[gallery] unmocked command ${command}`, args);
  }
  return null as T;
}

export async function mockListen(event: string, handler: Listener): Promise<() => void> {
  const set = listeners.get(event) ?? new Set<Listener>();
  set.add(handler);
  listeners.set(event, set);
  return () => set.delete(handler);
}

export function emitMockEvent(event: string, payload: unknown): void {
  for (const listener of listeners.get(event) ?? []) listener({ event, id: 0, payload });
}

declare global {
  interface Window {
    __galleryUnmockedCommands?: () => string[];
  }
}

window.__galleryUnmockedCommands = () => [...unhandled];

type HttpHandler = (url: URL, init?: RequestInit) => unknown;
const httpRoutes = new Map<string, HttpHandler>();
const GALLERY_HOST = ".gallery.test";

/** Answers daemon HTTP calls to `origin + path` with the handler's JSON (or its Response, as is). */
export function mockHttp(origin: string, path: string, handler: HttpHandler): void {
  httpRoutes.set(`${origin}${path}`, handler);
}

const nativeFetch = window.fetch.bind(window);
// Gallery daemon origins never reach the network: unmocked paths answer 404 like a daemon without the route.
window.fetch = async (input, init) => {
  const url = new URL(input instanceof Request ? input.url : String(input), window.location.href);
  const route = httpRoutes.get(`${url.origin}${url.pathname}`);
  if (!route) return url.hostname.endsWith(GALLERY_HOST) ? new Response("", { status: 404 }) : nativeFetch(input, init);
  const body = await route(url, init);
  return body instanceof Response ? body : new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
};

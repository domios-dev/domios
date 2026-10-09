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

export async function mockInvoke<T>(command: string, args?: Args): Promise<T> {
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

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { SecretBackendDetail } from "./SecretBackendDetail";
import type { CatalogEntry } from "./pluginCatalog";
import type { AppConfigShape } from "./shared";

// Deliberately not an `asserts` predicate: narrowing a counter to the literal
// it was just checked against turns the next count check into a type error.
function assert(condition: unknown, message: string): void {
  if (!condition) throw new Error(message);
}

// Kept as a local literal (not imported) so this test still loads and runs its render-count assertions even against a source revert that predates the constant's introduction.
const STATUS_REFRESH_DELAY_MS = 5000;

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "HTMLElement", "SVGElement", "Element", "Node", "Event"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never as Record<string, unknown>)[key], configurable: true });
  }
  // apiBaseOf() runs its IS_WEB branch under happy-dom (window is defined,
  // no __TAURI_INTERNALS__) and reads a token via localStorage.
  Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
}

// Mock only the polling timer so React's own scheduler continues to use real timing.
function installDelayTimerFake(delayMs: number) {
  const realSetTimeout = globalThis.setTimeout;
  const realClearTimeout = globalThis.clearTimeout;
  const pending: Array<() => void> = [];

  globalThis.setTimeout = ((cb: TimerHandler, delay?: number, ...args: unknown[]) => {
    if (delay === delayMs) {
      pending.push(cb as () => void);
      return pending[pending.length - 1] as unknown as ReturnType<typeof setTimeout>;
    }
    return realSetTimeout(cb as TimerHandler, delay, ...args);
  }) as typeof setTimeout;

  globalThis.clearTimeout = ((id: unknown) => {
    const idx = pending.indexOf(id as () => void);
    if (idx !== -1) {
      pending.splice(idx, 1);
      return;
    }
    realClearTimeout(id as Parameters<typeof clearTimeout>[0]);
  }) as typeof clearTimeout;

  return {
    pendingCount: () => pending.length,
    flushOne: () => {
      const cb = pending.shift();
      if (!cb) return false;
      cb();
      return true;
    },
    restore: () => {
      globalThis.setTimeout = realSetTimeout;
      globalThis.clearTimeout = realClearTimeout;
    },
  };
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function entry(): CatalogEntry {
  return {
    id: "bitwarden",
    displayName: "Bitwarden",
    installed: true,
    inMarketplace: true,
    system: false,
    verified: true,
    removable: true,
    dev: false,
    compatible: true,
    updateAvailable: false,
  };
}

async function main() {
  setupDom();
  const timer = installDelayTimerFake(STATUS_REFRESH_DELAY_MS);

  // An object, not three `let`s: assert() is an `asserts` predicate, so it narrows a bare binding to the literal it just checked and the next count check becomes a type error instead of a test.
  const calls = { status: 0, orgs: 0, colls: 0 };
  let statusGate = deferred<void>();

  globalThis.fetch = (async (url: string | URL) => {
    const u = String(url);
    if (u.includes("/api/secrets/status")) {
      calls.status++;
      await statusGate.promise;
      return { ok: true, json: async () => ({ backend: "local", status: { status: "unlocked" } }) } as Response;
    }
    if (u.includes("/api/secrets/organizations")) {
      calls.orgs++;
      return { ok: true, json: async () => ({ organizations: [] }) } as Response;
    }
    if (u.includes("/api/secrets/collections")) {
      calls.colls++;
      return { ok: true, json: async () => ({ collections: [] }) } as Response;
    }
    return { ok: true, json: async () => ({}) } as Response;
  }) as typeof fetch;

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);

  const config: AppConfigShape = {
    api: { host: "127.0.0.1", port: 7686, token: "tok-a" },
    integrations: { secrets: { backend: "local" } },
  };
  const catalogEntry = entry();

  const renderOnce = () =>
    act(async () => {
      root.render(<SecretBackendDetail entry={catalogEntry} config={config} />);
    });
  // Drains already-scheduled microtasks (promise chains already triggered),
  // never real elapsed time — a generous hop count is just settlement margin.
  const flush = (hops = 10) =>
    act(async () => {
      for (let i = 0; i < hops; i++) await Promise.resolve();
    });

  await renderOnce();
  await flush();

  assert(calls.status === 1, `mount must issue exactly one status fetch, got ${calls.status}`);
  assert(timer.pendingCount() === 0, "no refresh may be scheduled while the first status fetch is still in flight");

  // Stable callback inputs prevent unresolved status polling from tearing down and restarting on every parent render.
  await renderOnce();
  await renderOnce();
  await renderOnce();
  await flush();

  assert(calls.status === 1, `re-rendering with unchanged config must not issue additional status fetches, got ${calls.status}`);
  assert(timer.pendingCount() === 0, "re-rendering must not start a second refresh while one is in flight");

  // Schedule the next refresh from the completed request so only one follow-up timer exists.
  const resolveFirstStatus = statusGate.resolve;
  statusGate = deferred<void>();
  resolveFirstStatus();
  await flush();

  assert(calls.orgs === 1, `organizations effect must fire once when unlocked, got ${calls.orgs}`);
  assert(calls.colls === 1, `collections effect must fire once when unlocked, got ${calls.colls}`);
  assert(timer.pendingCount() === 1, `exactly one next-refresh timer must be scheduled after completion, got ${timer.pendingCount()}`);

  // Re-render again with unchanged config now that we're unlocked: the
  // organizations/collections effects must not re-fire.
  await renderOnce();
  await renderOnce();
  await flush();
  assert(calls.orgs === 1, `organizations effect must not re-fire on unchanged config across re-renders, got ${calls.orgs}`);
  assert(calls.colls === 1, `collections effect must not re-fire on unchanged config across re-renders, got ${calls.colls}`);

  // A backend slower than STATUS_REFRESH_DELAY_MS must not be issued a third call before the second one completes — assert no timer is queued while it's in flight (the self-throttle under test).
  assert(timer.flushOne(), "the scheduled timer callback must exist and be runnable");
  await flush();

  assert(calls.status === 2, `the scheduled timer must start exactly one new refresh, got ${calls.status}`);
  assert(timer.pendingCount() === 0, "no third refresh may be scheduled while the second one is still in flight");
  assert(!timer.flushOne(), "there must be nothing to flush while a refresh is in flight");

  statusGate.resolve();
  await flush();
  assert(calls.status === 2, "resolving the second refresh must not, by itself, start a third");
  assert(timer.pendingCount() === 1, "completion of the second refresh must schedule exactly the next one");
  assert(calls.orgs === 1 && calls.colls === 1, "a repeat 'unlocked' status must not re-fire the organizations/collections effects");

  await act(async () => root.unmount());
  timer.restore();
  console.log("SecretBackendDetail polling tests: 16/16 assertions passed");
}

void main();

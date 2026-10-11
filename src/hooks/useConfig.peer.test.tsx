import assert from "node:assert/strict";
import { register } from "node:module";
import { act, useLayoutEffect } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { withFailureDeadline } from "../test/withFailureDeadline";
import type { AppConfig } from "../lib/types";

register(`data:text/javascript,${encodeURIComponent(`
const leaves = {
  './UpdateSection': ['UpdateSection'], './ShellDiagnosticsBanner': ['ShellDiagnosticsBanner'],
  './AgentsTab': ['AgentsOverview', 'AgentProviderScreen'], './GeneralAgentCard': ['GeneralAgentCard'],
  './NotificationsTab': ['NotificationsOverview'], './MessengersCard': ['MessengersCard'],
  './SecretsTab': ['SecretsTab'], './LogsTab': ['LogsTab'], '../../help/HelpPanel': ['HelpPanel'],
};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith('/SettingsPanel.tsx')) {
    const names = leaves[specifier];
    if (names) return { url: 'data:text/javascript,' + encodeURIComponent(names.map(name => 'export const ' + name + ' = () => null;').join('')), shortCircuit: true };
    if (specifier.endsWith('.png')) return { url: 'data:text/javascript,export default "test-logo";', shortCircuit: true };
  }
  return next(specifier, context);
}
`)}`, import.meta.url);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
async function refused(promise: Promise<void>) {
  await assert.rejects(withFailureDeadline(promise, "config save refusal"), (error: unknown) =>
    error instanceof Error && (error.name === "AbortError" || error.message === "Config is not ready for this peer"));
}
function queue<T>() {
  const entries: T[] = [];
  let waiter: ((value: T) => void) | undefined;
  return {
    push(value: T) { if (waiter) { const done = waiter; waiter = undefined; done(value); } else entries.push(value); },
    next(label: string) { return withFailureDeadline(entries.length ? Promise.resolve(entries.shift()!) : new Promise<T>((done) => { waiter = done; }), label); },
  };
}

const A = "opaque-a-7f20";
const B = "opaque-b-91de";
type ConfigHook = ReturnType<typeof import("./useConfig").useConfig>;
type Request = {
  destination: string; method: string; body: AppConfig | null; signal: AbortSignal;
  snapshot: AppConfig; release: () => void; accept: () => void; fail: () => void;
};
type Stream = { destination: string; controller: ReadableStreamDefaultController<Uint8Array> };

async function fixture(local = false) {
  const win = new Window({ url: "http://gateway:7685" });
  const originalFetch = globalThis.fetch;
  const descriptors = new Map<string, PropertyDescriptor | undefined>();
  for (const key of ["window", "document", "navigator", "localStorage", "sessionStorage", "HTMLElement", "HTMLInputElement", "HTMLButtonElement", "HTMLSelectElement", "Element", "Node", "Event", "CustomEvent"]) {
    descriptors.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, value: win[key as keyof Window] });
  }
  const actDescriptor = Object.getOwnPropertyDescriptor(globalThis, "IS_REACT_ACT_ENVIRONMENT");
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const api = await import("../lib/webApi");
  const events = await import("../lib/webEvents");
  const bridge = await import("../lib/bridge");
  const { defaultConfig, useConfig } = await import("./useConfig");
  const { SettingsPanel } = await import("../components/Settings/SettingsPanel");
  const { WebPeerPicker } = await import("../components/WebPeerPicker/WebPeerPicker");
  const { MeshPresenceTray } = await import("../components/Titlebar/MeshPresenceTray");
  api.setTokenFor(api.selfHost(), "gateway-auth-sentinel");
  if (!local) api.setRelayTarget(A, "eight");
  const stores = new Map<string, AppConfig>();
  for (const [id, name, token] of [[A, "eight", "opaque-token-a"], [B, "rollacodebot", "opaque-token-b"]]) {
    const config = JSON.parse(JSON.stringify(defaultConfig())) as AppConfig;
    config.mesh.instance_name = name;
    config.mesh.enabled = true;
    config.api.token = token;
    config.font.family = `opaque-font-${id}`;
    config.integrations.telegram_allowed_users = [`opaque-user-${id}`];
    stores.set(id, config);
  }
  const initialA = structuredClone(stores.get(A)!);
  const initialB = structuredClone(stores.get(B)!);
  const requests = queue<Request>();
  const streams = queue<Stream>();
  const allRequests: Request[] = [];
  const allStreams: Stream[] = [];
  let holdLocalBody = false;
  let bodyRelease: (() => void) | undefined;
  const bodyStarted = deferred<void>();
  globalThis.fetch = async (input, init) => {
    const url = new URL(String(input));
    if (url.pathname.includes("stream")) {
      const destination = url.searchParams.get("peer") ?? A;
      let controller!: Stream["controller"];
      const body = new ReadableStream<Uint8Array>({ start(value) { controller = value; } });
      const stream = { destination, controller };
      streams.push(stream); allStreams.push(stream);
      return new Response(body);
    }
    const envelope = url.pathname === "/api/mesh/relay-to"
      ? JSON.parse(String(init?.body)) as { target_peer_id: string; method: string; path: string; body: AppConfig | null }
      : null;
    const destination = envelope?.target_peer_id ?? A;
    const method = envelope?.method ?? init?.method ?? "GET";
    const path = envelope?.path ?? url.pathname;
    assert.equal(new Headers(init?.headers).get("Authorization"), "Bearer gateway-auth-sentinel");
    if (path === "/api/mesh/peers") {
      return Response.json({ local_instance_name: local ? "eight" : "gateway", paired: (local ? [B] : [A, B]).map((id) => ({
        instance_id: id, instance_name: stores.get(id)!.mesh.instance_name, addresses: [], api_port: 7685,
        online: true, relay_protocol_version: 1,
      })) });
    }
    if (path === "/api/config") {
      const reply = deferred<Response>();
      const snapshot = structuredClone(stores.get(destination)!);
      snapshot.api.token = null;
      const body = envelope?.body ?? (init?.body ? JSON.parse(String(init.body)) as AppConfig : null);
      const result = method === "GET" ? snapshot : { ok: true };
      const request: Request = {
        destination, method, body, snapshot, signal: init!.signal!,
        accept() {
          assert.equal(method, "PATCH");
          assert(body);
          const previous = stores.get(destination)!;
          const updated = structuredClone({ ...previous, ...body });
          // The server owns its token; browser auth is the gateway credential.
          updated.api.token = previous.api.token;
          stores.set(destination, updated);
        },
        fail() {
          reply.resolve(Response.json(envelope ? { ok: false, status: 500, data: {}, error: "fixture refusal" } : {}, { status: envelope ? 200 : 500 }));
        },
        release() {
          if (!envelope && method === "GET" && holdLocalBody) {
            const stream = new ReadableStream<Uint8Array>({ start(controller) {
              bodyRelease = () => { controller.enqueue(new TextEncoder().encode(JSON.stringify(result))); controller.close(); };
            } });
            reply.resolve(new Response(stream));
            bodyStarted.resolve();
          } else reply.resolve(Response.json(envelope ? { ok: true, status: 200, data: result, error: null } : result));
        },
      };
      if (method === "PATCH") request.accept();
      allRequests.push(request); requests.push(request);
      return reply.promise;
    }
    const result = path === "/api/themes" ? { themes: ["github-dark", "dracula"] } : [];
    return Response.json(envelope ? { ok: true, status: 200, data: result, error: null } : result);
  };
  let current!: ConfigHook;
  const snapshots: ConfigHook[] = [];
  function App() {
    const config = useConfig();
    useLayoutEffect(() => { current = config; snapshots.push(config); });
    return <><WebPeerPicker /><SettingsPanel config={config.config} onUpdateConfig={config.updateConfig} onClose={() => {}} initialSection="general" /><MeshPresenceTray /></>;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  async function done() {
    await act(async () => root.unmount());
    events.__resetWebEventsForTest();
    for (const stream of allStreams) { try { stream.controller.close(); } catch { /* already canceled */ } }
    holdLocalBody = false; bodyRelease?.();
    for (const request of allRequests) request.release();
    api.invalidateWebRequestContext();
    globalThis.fetch = originalFetch;
    host.remove();
    for (const [key, descriptor] of descriptors) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key);
    }
    if (actDescriptor) Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", actDescriptor); else Reflect.deleteProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT");
    await win.happyDOM.close();
  }
  try { await act(async () => root.render(<App />)); } catch (error) { await done(); throw error; }
  function button(selector: string) {
    const element = document.querySelector<HTMLButtonElement>(selector);
    assert(element, `missing ${selector}`);
    return element;
  }
  async function click(selector: string) { await act(async () => button(selector).click()); }
  async function switchTo(id: string | null) {
    await click('[data-testid="peer-picker-button"]');
    if (id === null) await click('[data-testid="peer-picker-self"]');
    else {
      const rows = [...document.querySelectorAll<HTMLButtonElement>('[data-testid="peer-picker-item"]')];
      const row = rows.find((entry) => entry.dataset.peerName === (id === A ? "eight" : "rollacodebot"));
      assert(row);
      await act(async () => row.click());
    }
  }
  async function release(request: Request) { await act(async () => request.release()); }
  async function load() {
    const first = await requests.next("config hook load");
    const second = await requests.next("presence load");
    assert.equal(first.method, "GET"); assert.equal(second.method, "GET");
    await release(first); await release(second);
    return { first, second };
  }
  async function saveToggle() { await click('[role="switch"][aria-label="Ligatures"]'); }
  async function persist() {
    const request = await requests.next("persisted config write");
    assert.equal(request.method, "PATCH");
    request.accept(); await release(request); return request;
  }
  async function deliver(stream: Stream, payload: AppConfig) {
    const done = deferred<void>();
    const unlisten = await bridge.listen("test-barrier", () => done.resolve());
    try {
      await act(async () => {
        stream.controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify({ event_type: "config-changed", data: payload })}\n\ndata: ${JSON.stringify({ event_type: "test-barrier", data: null })}\n\n`));
        await withFailureDeadline(done.promise, "config event delivered");
      });
    } finally { unlisten(); }
  }
  async function rows() {
    await click('[data-testid="peer-picker-button"]');
    const values = [...document.querySelectorAll<HTMLButtonElement>('[data-testid="peer-picker-item"]')].map((row) => ({ id: row.dataset.peerId, name: row.dataset.peerName, text: row.textContent }));
    await click('[data-testid="peer-picker-button"]');
    return values;
  }
  return {
    api, stores, initialA, initialB, requests, streams, allRequests, snapshots,
    current: () => current, switchTo, load, release, saveToggle, persist, deliver, rows,
    holdBody() { holdLocalBody = true; }, bodyStarted: bodyStarted.promise,
    async finishBody() {
      assert(bodyRelease);
      const release = bodyRelease; bodyRelease = undefined;
      await act(async () => release());
    },
    async presenceOffline() {
      const trigger = [...document.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.title.startsWith("Mesh:"));
      assert(trigger); await act(async () => trigger.click());
      const option = [...document.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.startsWith("Offline"));
      assert(option); await act(async () => option.click());
    },
    done,
  };
}

let selectedCases = 0;
async function run(name: string, test: (f: Awaited<ReturnType<typeof fixture>>) => Promise<void>, local = false) {
  if (process.env.CONFIG_PEER_CASE && !name.includes(process.env.CONFIG_PEER_CASE)) return;
  selectedCases += 1;
  const f = await fixture(local);
  try { await test(f); console.log(`PASS ${name}`); } finally { await f.done(); }
}

async function main() {
  await run("Settings autosave cannot persist A config into B after the real picker switch", async (f) => {
    await f.load();
    await f.saveToggle();
    const positive = await f.persist();
    assert.equal(positive.destination, A);
    assert.deepEqual(f.stores.get(A), { ...f.initialA, font: { ...f.initialA.font, ligatures: false } });
    assert.deepEqual(f.stores.get(B), f.initialB);
    const staleSave = f.current().updateConfig;
    const staleConfig = f.current().config;
    await f.switchTo(B);
    await f.saveToggle();
    assert.deepEqual(f.stores.get(B), f.initialB, "CAUSAL RED: persisted B stays independent after a stale Settings autosave");
    assert.equal(f.allRequests.filter((request) => request.method === "PATCH" && request.destination === B).length, 0, "CAUSAL RED: stale Settings autosave must never start a B write");
    await refused(staleSave(staleConfig));
    await f.load();
    await f.saveToggle();
    const current = await f.persist();
    assert.equal(current.destination, B);
    assert.equal(current.body!.api.token, "gateway-auth-sentinel");
    assert.deepEqual(f.stores.get(B), { ...f.initialB, font: { ...f.initialB.font, ligatures: false } });
    assert.deepEqual(f.stores.get(A), { ...f.initialA, font: { ...f.initialA.font, ligatures: false } });
    assert.deepEqual((await f.rows()).map(({ id, name }) => ({ id, name })), [{ id: A, name: "eight" }, { id: B, name: "rollacodebot" }]);
  });
  await run("loading and failed config contexts refuse defaults and events cannot enable saving", async (f) => {
    const loadingSave = f.current().updateConfig;
    await refused(loadingSave(f.current().config));
    await f.saveToggle();
    const read = await f.requests.next("failed hook read");
    const otherRead = await f.requests.next("failed presence read");
    await act(async () => { read.fail(); otherRead.fail(); });
    assert.equal(f.current().loading, false);
    await refused(f.current().updateConfig(f.current().config));
    const stream = await f.streams.next("event after failed read");
    await f.deliver(stream, f.initialA);
    await f.saveToggle();
    assert.equal(f.allRequests.filter((request) => request.method === "PATCH").length, 0);
    assert.deepEqual(f.stores.get(A), f.initialA); assert.deepEqual(f.stores.get(B), f.initialB);
    await f.switchTo(B); await f.load();
    await refused(loadingSave(f.current().config));
    await f.saveToggle(); await f.persist();
    assert.deepEqual(f.stores.get(B), { ...f.initialB, font: { ...f.initialB.font, ligatures: false } });
  });
  await run("accepted pre-switch saves stay pinned and late completion cannot succeed for B", async (f) => {
    await f.load();
    const old = f.current();
    const next = { ...old.config, font: { ...old.config.font, size: 19 } };
    const outcome = old.updateConfig(next).then(() => "saved", () => "stale");
    const pending = await f.requests.next("A save accepted before switch");
    assert.equal(pending.destination, A); pending.accept();
    await f.switchTo(B);
    assert(pending.signal.aborted);
    await f.release(pending);
    assert.equal(await outcome, "stale");
    await f.load();
    assert.deepEqual(f.stores.get(A), { ...f.initialA, font: { ...f.initialA.font, size: 19 } });
    assert.deepEqual(f.stores.get(B), f.initialB);
    assert.equal(f.current().config.mesh.instance_name, "rollacodebot");
  });
  await run("rapid A B A refuses old callbacks, reads and old event stream frames", async (f) => {
    await f.load();
    const old = f.current();
    const oldStream = await f.streams.next("A events");
    await f.switchTo(B);
    const bRead = await f.requests.next("B config");
    const bPresence = await f.requests.next("B presence");
    await f.switchTo(A);
    await f.release(bRead); await f.release(bPresence);
    await f.load();
    await refused(old.updateConfig(old.config));
    const bStream = await f.streams.next("B events");
    const newStream = await f.streams.next("new A events");
    const alien = { ...f.initialB, mesh: { ...f.initialB.mesh, instance_name: "alien" } };
    assert.throws(() => oldStream.controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify({ event_type: "config-changed", data: alien })}\n\n`)));
    assert.throws(() => bStream.controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify({ event_type: "config-changed", data: alien })}\n\n`)));
    await f.deliver(newStream, { ...f.initialA, api: { ...f.initialA.api, token: null } });
    assert.equal(f.current().config.mesh.instance_name, "eight");
    await f.saveToggle(); await f.persist();
    assert.deepEqual(f.stores.get(A), { ...f.initialA, font: { ...f.initialA.font, ligatures: false } });
    assert.deepEqual(f.stores.get(B), f.initialB);
  });
  await run("Presence read modify write refuses a local A body completing after switch to B", async (f) => {
    await f.load();
    await f.presenceOffline();
    const positiveRead = await f.requests.next("same peer presence read");
    await f.release(positiveRead);
    const positive = await f.persist(); assert.equal(positive.destination, A);
    assert.deepEqual(f.stores.get(A), { ...f.initialA, mesh: { ...f.initialA.mesh, enabled: false } });
    // Toggle back through the actual tray so the held transaction changes a field.
    const stream = await f.streams.next("local A events");
    await f.deliver(stream, f.stores.get(A)!);
    await f.deliver(stream, f.initialA);
    f.holdBody();
    await f.presenceOffline();
    const pending = await f.requests.next("held local presence body");
    await f.release(pending); await f.bodyStarted;
    await f.switchTo(B);
    await f.finishBody();
    assert.deepEqual(f.stores.get(B), f.initialB, "CAUSAL RED: held A presence transaction must not replace persisted B");
    assert.equal(f.allRequests.filter((request) => request.method === "PATCH" && request.destination === B).length, 0);
    assert.deepEqual(f.stores.get(A), { ...f.initialA, mesh: { ...f.initialA.mesh, enabled: false } });
  }, true);
  assert(selectedCases > 0, "CONFIG_PEER_CASE must select a real test");
  console.log(`PASS config peer persistence: ${selectedCases} cases`);
}

void main().catch((error) => { console.error(error); process.exitCode = 1; });

import { afterEach, describe, expect, it, vi } from "vitest";
const bindings = vi.hoisted(() => ({ output: vi.fn(), events: vi.fn(), remote: vi.fn(), native: vi.fn(), orchestration: vi.fn() }));
vi.mock("./outputStream", () => ({ bindOutputStreamTransport: bindings.output }));
vi.mock("./webEvents", () => ({ bindWebEventsTransport: bindings.events }));
vi.mock("./remoteTabEvents", () => ({ bindRemoteTabEventsTransport: bindings.remote }));
vi.mock("./nativeBridge", () => ({ bindNativeBridgeTransport: bindings.native }));
vi.mock("./transport/orchestrationRuntime", () => ({ bindOrchestrationTransport: bindings.orchestration }));
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); vi.resetModules(); });
describe("shared daemon runtime", () => {
  it("binds the three base streams and orchestration at every replacement", async () => {
    vi.stubGlobal("window", { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {}, dispatchEvent: vi.fn() });
    const { selectedDaemonTransport, ensureDaemonRuntime } = await import("./daemonRuntime");
    ensureDaemonRuntime();
    const { configureDaemonTransport } = await import("./daemonBootstrap");
    const first = configureDaemonTransport({ origin: "http://127.0.0.1:7697", token: "first" });
    const second = configureDaemonTransport({ origin: "http://127.0.0.1:7698", token: "second" });
    for (const bind of [bindings.output, bindings.events, bindings.remote, bindings.native]) expect(bind.mock.calls).toEqual([[first], [second]]);
    expect(bindings.orchestration.mock.calls).toEqual([[selectedDaemonTransport], [selectedDaemonTransport]]);
  });
  it("preserves a request started before initial binding and aborts it on replacement", async () => {
    vi.stubGlobal("window", { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {}, dispatchEvent: vi.fn() });
    vi.stubGlobal("localStorage", { getItem: () => null });
    let finish!: (response: Response) => void;
    let signal!: AbortSignal;
    vi.stubGlobal("fetch", vi.fn((_input: unknown, options: RequestInit) => {
      signal = options.signal as AbortSignal;
      return new Promise<Response>(resolve => { finish = resolve; });
    }));
    const { ensureDaemonRuntime } = await import("./daemonRuntime");
    const { configureDaemonTransport } = await import("./daemonBootstrap");
    const { webJson } = await import("./webApi");
    configureDaemonTransport({ origin: "http://127.0.0.1:7697", token: "first" });
    const read = webJson("GET", "/api/config");
    ensureDaemonRuntime();
    expect(signal.aborted).toBe(false);
    finish(Response.json({ value: "current" }));
    await expect(read).resolves.toEqual({ value: "current" });
    const staleRead = webJson("GET", "/api/config");
    configureDaemonTransport({ origin: "http://127.0.0.1:7698", token: "second" });
    expect(signal.aborted).toBe(true);
    finish(Response.json({ value: "stale" }));
    await expect(staleRead).rejects.toMatchObject({ name: "AbortError" });
  });
  it("uses the base for explicit peer streams and selected relay for domain requests", async () => {
    const fetch = vi.fn().mockResolvedValue(Response.json({ ok: true, status: 200, data: { result: "peer" } }));
    vi.stubGlobal("fetch", fetch);
    vi.stubGlobal("window", { location: { origin: "http://127.0.0.1:7697" }, dispatchEvent: vi.fn(), CustomEvent: class {} });
    vi.stubGlobal("localStorage", { getItem: (key: string) => key.includes("relay-peer:") ? "peer" : null });
    const { selectedDaemonTransport, ensureDaemonRuntime } = await import("./daemonRuntime");
    ensureDaemonRuntime();
    const { configureDaemonTransport } = await import("./daemonBootstrap");
    const base = configureDaemonTransport({ origin: "http://127.0.0.1:7697", token: "owner" });
    expect(await selectedDaemonTransport.json("GET", "/api/config")).toEqual({ result: "peer" });
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe("http://127.0.0.1:7697/api/mesh/relay-to");
    expect(JSON.parse(init.body)).toEqual({ target_peer_id: "peer", method: "GET", path: "/api/config", body: null });
    expect(bindings.events).toHaveBeenCalledWith(base);
  });
});

import { afterEach, describe, expect, it, vi } from "vitest";
const bindings = vi.hoisted(() => ({ output: vi.fn(), events: vi.fn(), remote: vi.fn(), native: vi.fn(), orchestration: vi.fn() }));
vi.mock("./outputStream", () => ({ bindOutputStreamTransport: bindings.output }));
vi.mock("./webEvents", () => ({ bindWebEventsTransport: bindings.events }));
vi.mock("./remoteTabEvents", () => ({ bindRemoteTabEventsTransport: bindings.remote }));
vi.mock("./nativeBridge", () => ({ bindNativeBridgeTransport: bindings.native }));
vi.mock("./transport/orchestrationRuntime", () => ({ configureOrchestrationTransport: bindings.orchestration }));
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); vi.resetModules(); });
describe("shared daemon runtime", () => {
  it("binds the three base streams and orchestration at every replacement", async () => {
    vi.stubGlobal("window", { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {} });
    const { selectedDaemonTransport } = await import("./daemonRuntime");
    const { configureDaemonTransport } = await import("./daemonBootstrap");
    const first = configureDaemonTransport({ origin: "http://127.0.0.1:7697", token: "first" });
    const second = configureDaemonTransport({ origin: "http://127.0.0.1:7698", token: "second" });
    for (const bind of [bindings.output, bindings.events, bindings.remote, bindings.native]) expect(bind.mock.calls).toEqual([[first], [second]]);
    expect(bindings.orchestration.mock.calls).toEqual([[selectedDaemonTransport], [selectedDaemonTransport]]);
  });
  it("uses the base for explicit peer streams and selected relay for domain requests", async () => {
    const fetch = vi.fn().mockResolvedValue(Response.json({ ok: true, status: 200, data: { result: "peer" } }));
    vi.stubGlobal("fetch", fetch);
    vi.stubGlobal("window", { location: { origin: "http://127.0.0.1:7697" }, dispatchEvent: vi.fn(), CustomEvent: class {} });
    vi.stubGlobal("localStorage", { getItem: (key: string) => key.includes("relay-peer:") ? "peer" : null });
    const { selectedDaemonTransport } = await import("./daemonRuntime");
    const { configureDaemonTransport } = await import("./daemonBootstrap");
    const base = configureDaemonTransport({ origin: "http://127.0.0.1:7697", token: "owner" });
    expect(await selectedDaemonTransport.json("GET", "/api/config")).toEqual({ result: "peer" });
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe("http://127.0.0.1:7697/api/mesh/relay-to");
    expect(JSON.parse(init.body)).toEqual({ target_peer_id: "peer", method: "GET", path: "/api/config", body: null });
    expect(bindings.events).toHaveBeenCalledWith(base);
  });
});

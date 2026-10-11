import { afterEach, describe, expect, it, vi } from "vitest";
import { readReloadCredentials, validateDaemonOrigin } from "./daemonBootstrap";

afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });
describe("explicit daemon bootstrap", () => {
  it("allows only the injected exact native development origin", async () => {
    const origin = "http://127.0.0.1:7690";
    const frontendOrigin = "http://127.0.0.1:1421";
    expect(validateDaemonOrigin(origin, frontendOrigin, true, frontendOrigin)).toBe(origin);
    expect(() => validateDaemonOrigin(origin, "http://127.0.0.1:1422", true, frontendOrigin)).toThrow();
    expect(() => validateDaemonOrigin(origin, frontendOrigin, false, frontendOrigin)).toThrow();
    expect(() => validateDaemonOrigin(origin, "https://foreign.example", true, "https://foreign.example")).toThrow();
    const shell = { location: { origin: frontendOrigin }, __TAURI_INTERNALS__: {},
      __CODETERM_DAEMON__: { origin, token: "dev-token", frontendOrigin } };
    vi.stubGlobal("window", shell);
    const { initializeDaemonTransport } = await import("./daemonBootstrap");
    expect(initializeDaemonTransport()!.origin()).toBe(origin);
    expect(shell).not.toHaveProperty("__CODETERM_DAEMON__");
  });
  it("requires an HTTP origin and scopes shell credentials to loopback and the page origin", () => {
    for (const origin of ["tauri://localhost", "http://127.0.0.1/path", "http://user:secret@127.0.0.1", "https://127.0.0.1/?token=x"]) expect(() => validateDaemonOrigin(origin)).toThrow();
    expect(() => validateDaemonOrigin("https://remote.example", undefined, true)).toThrow("loopback");
    expect(() => validateDaemonOrigin("http://127.0.0.1:7686", "https://foreign.example", true)).toThrow("another origin");
    expect(validateDaemonOrigin("http://127.0.0.1:7686", "https://tauri.localhost", true)).toBe("http://127.0.0.1:7686");
    expect(validateDaemonOrigin("http://127.0.0.1:7686", "tauri://localhost", true)).toBe("http://127.0.0.1:7686");
  });
  it("consumes and deletes the native global without deriving transport from shell capability", async () => {
    const shell = { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {}, __CODETERM_DAEMON__: { origin: "http://127.0.0.1:7697", token: "shell-token" } };
    vi.stubGlobal("window", shell);
    const { initializeDaemonTransport } = await import("./daemonBootstrap");
    const client = initializeDaemonTransport()!;
    expect(client.origin()).toBe("http://127.0.0.1:7697");
    expect(client.currentToken()).toBe("shell-token");
    expect(shell).not.toHaveProperty("__CODETERM_DAEMON__");
  });
  it("restores the post-swap BootContext while preserving the observer ticket", async () => {
    const context = { apiOrigin: "http://127.0.0.1:7697", token: "swap-token", clientId: "client", ticket: { token: "health-ticket" } };
    const getItem = vi.fn(() => JSON.stringify(context));
    vi.stubGlobal("window", { location: { origin: context.apiOrigin }, __TAURI_INTERNALS__: {}, sessionStorage: { getItem } });
    const { initializeDaemonTransport } = await import("./daemonBootstrap");
    expect(initializeDaemonTransport()!.currentToken()).toBe("swap-token");
    expect(getItem).toHaveBeenCalledWith("codeterm.frontend.boot");
    expect(readReloadCredentials({ getItem })).toEqual({ apiOrigin: context.apiOrigin, token: "swap-token" });
    expect(context.ticket.token).toBe("health-ticket");
  });
  it("prefers swap credentials and consumes the shell injection after a reload", async () => {
    const shell = {
      location: { origin: "http://127.0.0.1:7697" }, __TAURI_INTERNALS__: {},
      __CODETERM_DAEMON__: { origin: "http://127.0.0.1:7697", token: "initial-shell-token" },
      sessionStorage: { getItem: () => JSON.stringify({ apiOrigin: "http://127.0.0.1:7697", token: "swap-token" }) },
    };
    vi.stubGlobal("window", shell);
    const { initializeDaemonTransport } = await import("./daemonBootstrap");
    expect(initializeDaemonTransport()!.currentToken()).toBe("swap-token");
    expect(shell).not.toHaveProperty("__CODETERM_DAEMON__");
  });
  it("accepts BootContext directly and rebinds clients on replacement", async () => {
    vi.stubGlobal("window", { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {} });
    const { configureDaemonTransport, onDaemonTransport } = await import("./daemonBootstrap");
    const bound = vi.fn();
    const stop = onDaemonTransport(bound);
    configureDaemonTransport({ apiOrigin: "http://127.0.0.1:7697", token: "one" });
    configureDaemonTransport({ origin: "http://127.0.0.1:7698", token: "two" });
    expect(bound.mock.calls.map(([client]) => [client.origin(), client.currentToken()])).toEqual([["http://127.0.0.1:7697", "one"], ["http://127.0.0.1:7698", "two"]]);
    stop();
  });
  it("fails honestly when a native fallback has no injected or restored origin", async () => {
    vi.stubGlobal("window", { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {}, sessionStorage: { getItem: () => null } });
    const { baseDaemonTransport } = await import("./daemonBootstrap");
    expect(() => baseDaemonTransport()).toThrow("Explicit daemon bootstrap required");
  });
  it("reads browser token changes without replacing the shell or retaining an old token", async () => {
    let token: string | null = null;
    vi.stubGlobal("window", { location: { origin: "https://gateway.example" }, sessionStorage: { getItem: () => null }, localStorage: { getItem: () => token } });
    const { baseDaemonTransport } = await import("./daemonBootstrap");
    const client = baseDaemonTransport();
    expect(client.currentToken()).toBeNull();
    token = "login-token";
    expect(client.currentToken()).toBe("login-token");
  });
});

it("boots the real native runtime graph from Vite and acknowledges readiness only over daemon HTTP", async () => {
  vi.stubGlobal("__CODETERM_DEV__", true);
  const diagnostic = vi.fn();
  const eventTarget = new EventTarget();
  const shell = { __CODETERM_SHELL_DIAGNOSTIC__: diagnostic, location: { origin: "http://127.0.0.1:1463", pathname: "/" }, __TAURI_INTERNALS__: {},
    __CODETERM_DAEMON__: { origin: "http://127.0.0.1:7690", token: "dev-fixture", frontendOrigin: "http://127.0.0.1:1463" },
    sessionStorage: { getItem: () => null }, localStorage: { getItem: () => null },
    CustomEvent, dispatchEvent: eventTarget.dispatchEvent.bind(eventTarget), addEventListener: eventTarget.addEventListener.bind(eventTarget), removeEventListener: eventTarget.removeEventListener.bind(eventTarget),
  };
  vi.stubGlobal("window", shell);
  const requests: { url: string; init?: RequestInit }[] = [];
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    requests.push({ url, init });
    return new Response(JSON.stringify(url.endsWith("/bootstrap") ? { generation: 7 } : true), { headers: { "Content-Type": "application/json" } });
  });
  const { ensureDaemonRuntime } = await import("./daemonRuntime");
  for (let access = 0; access < 102; access++) ensureDaemonRuntime();
  const { invoke } = await import("./bridge");
  const generation = await invoke<number>("frontend_readiness_generation");
  expect(await invoke<boolean>("frontend_ready", { generation })).toBe(true);
  expect(requests.map(({ url }) => url)).toEqual(["http://127.0.0.1:7690/api/frontend/bootstrap", "http://127.0.0.1:7690/api/frontend/ready"]);
  expect(JSON.parse(String(requests[1].init?.body))).toEqual({ generation: 7 });
  expect(new Headers(requests[1].init?.headers).get("Authorization")).toBe("Bearer dev-fixture");
  expect(shell).not.toHaveProperty("__CODETERM_DAEMON__");
  expect(diagnostic.mock.calls.filter(([stage]) => stage === "runtime-bound")).toHaveLength(1);
});

it("preserves base and peer clients for stable configuration and binds once", async () => {
  vi.stubGlobal("window", { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {} });
  const { configureDaemonTransport, daemonTransportForPeer, onDaemonTransport } = await import("./daemonBootstrap");
  const bound = vi.fn();
  const stop = onDaemonTransport(bound);
  const configuration = { origin: "http://127.0.0.1:7697", token: "stable-token" };
  const first = configureDaemonTransport(configuration);
  const peer = daemonTransportForPeer("peer");
  for (let access = 0; access < 102; access++) expect(configureDaemonTransport(configuration)).toBe(first);
  expect(daemonTransportForPeer("peer")).toBe(peer);
  expect(bound).toHaveBeenCalledExactlyOnceWith(first);
  expect(configureDaemonTransport({ ...configuration, token: "changed-token" })).not.toBe(first);
  expect(bound).toHaveBeenCalledTimes(2);
  stop();
});

it("shares real page events across transport users and moves them to replacement credentials", async () => {
  vi.stubGlobal("window", { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {} });
  const signals: AbortSignal[] = [];
  const fetch = vi.fn<typeof globalThis.fetch>(async (_input, init) => {
    signals.push(init!.signal!);
    return new Response(new ReadableStream());
  });
  vi.stubGlobal("fetch", fetch);
  const { configureDaemonTransport } = await import("./daemonBootstrap");
  const configuration = { origin: "http://127.0.0.1:7690", token: "first" };
  const first = configureDaemonTransport(configuration);
  const stops = Array.from({ length: 20 }, () => first.subscribe("/api/events/stream", () => {}));
  expect(fetch).toHaveBeenCalledTimes(1);
  const next = configureDaemonTransport({ ...configuration, token: "second" });
  const stopNext = next.subscribe("/api/events/stream", () => {});
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(signals[0].aborted).toBe(true);
  expect(new Headers(fetch.mock.calls[1][1]?.headers).get("Authorization")).toBe("Bearer second");
  stops.forEach(stop => stop());
  expect(signals[1].aborted).toBe(false);
  stopNext();
  expect(signals[1].aborted).toBe(true);
});

it("shares peer events between the peer facade and explicit remote tab subscriptions", async () => {
  vi.stubGlobal("window", { location: { origin: "tauri://localhost" }, __TAURI_INTERNALS__: {} });
  const fetch = vi.fn<typeof globalThis.fetch>(async () => new Response(new ReadableStream()));
  vi.stubGlobal("fetch", fetch);
  const { configureDaemonTransport, daemonTransportForPeer } = await import("./daemonBootstrap");
  const base = configureDaemonTransport({ origin: "http://127.0.0.1:7690", token: "token" });
  const peer = daemonTransportForPeer("peer");
  const first = peer.subscribe("/api/events/stream?since=b%3A1", () => {});
  const second = base.subscribe("/api/mesh/relay-stream?peer=peer&path=%2Fapi%2Fevents%2Fstream", () => {});
  expect(fetch).toHaveBeenCalledTimes(1);
  first();
  second();
});

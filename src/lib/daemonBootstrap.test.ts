import { afterEach, describe, expect, it, vi } from "vitest";
import { readReloadCredentials, validateDaemonOrigin } from "./daemonBootstrap";

afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });
describe("explicit daemon bootstrap", () => {
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

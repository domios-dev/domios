import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vitest";
import type { TransportClient } from "../lib/transportClient";
import type { StartupPhase } from "../lib/serverReadiness";
const fixture = vi.hoisted(() => ({ listener: undefined as ((client: TransportClient) => void) | undefined, initialize: vi.fn(), wait: vi.fn(), stop: vi.fn() }));
vi.mock("../lib/daemonBootstrap", () => ({ FRONTEND_BOOT_KEY: "codeterm.frontend.boot", baseDaemonTransport: vi.fn(), onDaemonTransport: (listener: (client: TransportClient) => void) => { fixture.listener = listener; return fixture.stop; } }));
vi.mock("../lib/frontendUpdate/browser", () => ({ initializeBrowserFrontendUpdates: fixture.initialize }));
vi.mock("../lib/resizeOwner", () => ({ getResizeClientId: () => "client:stable-client" }));
vi.mock("../lib/serverReadiness", () => ({ waitForServerReady: fixture.wait }));
import { DaemonFrontendBootstrap } from "./DaemonFrontendBootstrap";

beforeEach(() => { fixture.initialize.mockReset(); fixture.wait.mockReset().mockResolvedValue(undefined); fixture.stop.mockClear(); fixture.listener = undefined; sessionStorage.clear(); });

it("hydrates before rendering App and reinitializes with replacement base credentials", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  sessionStorage.clear();
  let finish!: (value: { dispose: () => void }) => void;
  fixture.initialize.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  const host = document.createElement("div");
  const root = createRoot(host);
  const dispose = vi.fn();
  const first = {} as TransportClient;
  const second = {} as TransportClient;
  try {
    await act(async () => root.render(<DaemonFrontendBootstrap><button>App ready</button></DaemonFrontendBootstrap>));
    await act(async () => { fixture.listener!(first); });
    expect(host.querySelector("button")).toBeNull();
    expect(fixture.initialize).toHaveBeenCalledWith(first, expect.objectContaining({ clientId: "client-stable-client" }));
    await act(async () => finish({ dispose }));
    expect(host.querySelector("button")).not.toBeNull();
    const boot = { clientId: "reload-client", token: "secret", apiOrigin: "http://127.0.0.1:7686", ticket: { token: "health" }, bundled: { index: "/bundled/index.html", minApiVersion: 2, maxApiVersion: 2 } };
    sessionStorage.setItem("codeterm.frontend.boot", JSON.stringify(boot));
    await act(async () => { fixture.listener!(second); });
    expect(dispose).toHaveBeenCalledOnce();
    expect(host.querySelector("button")).toBeNull();
    expect(fixture.initialize).toHaveBeenLastCalledWith(second, expect.objectContaining({ clientId: "reload-client", boot }));
    await act(async () => finish({ dispose: vi.fn() }));
  } finally { await act(async () => root.unmount()); }
  expect(fixture.stop).toHaveBeenCalledOnce();
});


it("renders App after optional update initialization fails", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  fixture.initialize.mockRejectedValue(new Error("optional initializer failed"));
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<DaemonFrontendBootstrap><button>App ready</button></DaemonFrontendBootstrap>));
    await act(async () => { fixture.listener!({} as TransportClient); });
    expect(host.querySelector("button")).not.toBeNull();
  } finally { await act(async () => root.unmount()); }
});

it("disables swapping after a runtime failure while App remains rendered", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const dispose = vi.fn();
  fixture.initialize.mockResolvedValue({ dispose });
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<DaemonFrontendBootstrap><button>App ready</button></DaemonFrontendBootstrap>));
    await act(async () => { fixture.listener!({} as TransportClient); });
    const options = fixture.initialize.mock.calls[0][1] as { onError(error: unknown): void };
    await act(async () => options.onError(new Error("optional stream failed")));
    expect(host.querySelector("button")).not.toBeNull();
    expect(dispose).toHaveBeenCalledOnce();
  } finally { await act(async () => root.unmount()); }
});

it("waits for server readiness before hydration and app requests", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  let ready!: () => void;
  fixture.wait.mockImplementation(() => new Promise<void>(resolve => { ready = resolve; }));
  fixture.initialize.mockResolvedValue({ dispose: vi.fn() });
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<DaemonFrontendBootstrap><button>App ready</button></DaemonFrontendBootstrap>));
    await act(async () => { fixture.listener!({} as TransportClient); });
    expect(fixture.initialize).not.toHaveBeenCalled();
    expect(host.querySelector("button")).toBeNull();
    expect(host.querySelector("[role=status]")).not.toBeNull();
    await act(async () => ready());
    expect(fixture.initialize).toHaveBeenCalledOnce();
    expect(host.querySelector("button")).not.toBeNull();
  } finally { await act(async () => root.unmount()); }
});

it("keeps the app unmounted when server initialization fails", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  fixture.wait.mockRejectedValue(new Error("startup failed"));
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<DaemonFrontendBootstrap><button>App ready</button></DaemonFrontendBootstrap>));
    await act(async () => { fixture.listener!({} as TransportClient); });
    expect(fixture.initialize).not.toHaveBeenCalled();
    expect(host.querySelector("button")).toBeNull();
    expect(host.querySelector("[role=alert]")).not.toBeNull();
  } finally { await act(async () => root.unmount()); }
});

it("shows service and plan progress while keeping application requests gated", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  let publish!: (phase: StartupPhase | null) => void;
  let finish!: () => void;
  fixture.wait.mockImplementation((_client, _signal, onPhase) => {
    publish = onPhase;
    return new Promise<void>(resolve => { finish = resolve; });
  });
  fixture.initialize.mockResolvedValue({ dispose: vi.fn() });
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<DaemonFrontendBootstrap><button>App ready</button></DaemonFrontendBootstrap>));
    await act(async () => { fixture.listener!({} as TransportClient); });
    await act(async () => publish("service"));
    expect(host.querySelector("[role=status]")?.textContent).toContain("service");
    await act(async () => publish("plans"));
    expect(host.querySelector("[role=status]")?.textContent).toContain("plans");
    expect(fixture.initialize).not.toHaveBeenCalled();
    expect(host.querySelector("button")).toBeNull();
    await act(async () => finish());
    expect(host.querySelector("button")).not.toBeNull();
  } finally { await act(async () => root.unmount()); }
});

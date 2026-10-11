import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { TransportClient } from "../lib/transportClient";
const fixture = vi.hoisted(() => ({ listener: undefined as ((client: TransportClient) => void) | undefined, initialize: vi.fn(), stop: vi.fn() }));
vi.mock("../lib/daemonBootstrap", () => ({ FRONTEND_BOOT_KEY: "codeterm.frontend.boot", baseDaemonTransport: vi.fn(), onDaemonTransport: (listener: (client: TransportClient) => void) => { fixture.listener = listener; return fixture.stop; } }));
vi.mock("../lib/frontendUpdate/browser", () => ({ initializeBrowserFrontendUpdates: fixture.initialize }));
vi.mock("../lib/resizeOwner", () => ({ getResizeClientId: () => "stable-client" }));
import { DaemonFrontendBootstrap } from "./DaemonFrontendBootstrap";

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

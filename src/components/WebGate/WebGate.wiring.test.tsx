import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { WebGate } from "./WebGate";

const api = vi.hoisted(() => ({
  token: null as string | null,
  localJson: vi.fn(),
  setTokenFor: vi.fn(),
  clearRelayTarget: vi.fn(),
}));

vi.mock("../../lib/webApi", () => ({
  IS_WEB: true,
  apiHost: () => "https://app.test",
  selfHost: () => "https://app.test",
  currentToken: () => api.token,
  setTokenFor: (host: string, token: string) => {
    expect(window.location.hash).toBe("");
    expect(window.location.search).not.toContain("token=");
    api.token = token;
    api.setTokenFor(host, token);
  },
  localJson: (...args: unknown[]) => api.localJson(...args),
  relayTarget: () => null,
  clearRelayTarget: () => api.clearRelayTarget(),
  setRelayTarget: vi.fn(),
  usableRelayTarget: vi.fn(),
  WebApiError: class extends Error {},
}));
vi.mock("../../lib/bootCover", () => ({ releaseBootCover: vi.fn() }));

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  api.token = null;
  api.localJson.mockImplementation(() => {
    expect(window.location.hash).toBe("");
    return Promise.resolve([]);
  });
  window.history.replaceState(null, "", "/app?view=all");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

it.each(["hashchange", "popstate"])("consumes %s credentials while mounted and scrubs history before pending validation", async (type) => {
  await act(async () => root.render(<WebGate><main>Authenticated app</main></WebGate>));
  expect(host.querySelector('input[type="password"]')).not.toBeNull();
  let finish!: () => void;
  api.localJson.mockImplementation(() => {
    expect(window.location.hash).toBe("");
    return new Promise<void>((resolve) => { finish = resolve; });
  });
  const pushes = vi.spyOn(window.history, "pushState");
  window.history.replaceState({ custom: "preserved" }, "", "/app?view=all#token=fresh&relay=self");
  await act(async () => window.dispatchEvent(type === "popstate"
    ? new PopStateEvent(type, { state: window.history.state }) : new HashChangeEvent(type)));
  expect(api.token).toBe("fresh");
  expect(api.setTokenFor).toHaveBeenCalledTimes(1);
  expect(api.clearRelayTarget).toHaveBeenCalledOnce();
  expect(window.location.pathname + window.location.search + window.location.hash).toBe("/app?view=all");
  expect(window.history.state.custom).toBe("preserved");
  expect(pushes.mock.calls.every((call) => !String(call[2]).includes("token="))).toBe(true);
  expect(api.localJson).toHaveBeenCalledWith("GET", "/api/workspaces");
  await act(async () => window.dispatchEvent(new HashChangeEvent("hashchange")));
  expect(api.localJson).toHaveBeenCalledTimes(1);
  await act(async () => finish());
  expect(host.querySelector("main")?.textContent).toBe("Authenticated app");
});

it("scrubs the initial URL before arming the history guard and unregisters navigation listeners", async () => {
  const pushes = vi.spyOn(window.history, "pushState");
  window.history.replaceState(null, "", "/app?view=all&token=query#token=fragment");
  await act(async () => root.render(<WebGate><main>Authenticated app</main></WebGate>));
  expect(api.token).toBe("fragment");
  expect(host.querySelector("main")).not.toBeNull();
  expect(pushes.mock.calls.every((call) => !String(call[2]).includes("token="))).toBe(true);
  await act(async () => root.unmount());
  root = createRoot(host);
  window.history.replaceState(null, "", "/app#token=after-unmount");
  window.dispatchEvent(new HashChangeEvent("hashchange"));
  window.dispatchEvent(new PopStateEvent("popstate"));
  expect(api.token).toBe("fragment");
});

it("ignores an older validation failure after a newer token has connected", async () => {
  await act(async () => root.render(<WebGate><main>Authenticated app</main></WebGate>));
  let rejectOld!: (error: Error) => void;
  api.localJson.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectOld = reject; }));
  window.history.replaceState(window.history.state, "", "/app#token=old");
  await act(async () => window.dispatchEvent(new HashChangeEvent("hashchange")));
  window.history.replaceState(window.history.state, "", "/app#token=new");
  await act(async () => window.dispatchEvent(new HashChangeEvent("hashchange")));
  await act(async () => rejectOld(new Error("old request rejected")));
  expect(api.token).toBe("new");
  expect(host.querySelector("main")).not.toBeNull();
  expect(window.location.hash).toBe("");
});

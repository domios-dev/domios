import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Notification, Tab } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import { resetRemoteChatStatus, updateRemoteChatPeers } from "../../lib/remoteChatStatus";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { MeshPeersResponse } from "../../lib/types";

const fakes = vi.hoisted(() => ({ reconnect: vi.fn(async () => {}) }));
vi.mock("./tabSessionActions", async (importOriginal) => ({
  ...await importOriginal<typeof import("./tabSessionActions")>(),
  reconnectTabSession: fakes.reconnect,
}));
vi.mock("../Sidebar/ProviderSwitchDialog", () => ({ useProviderSwitchDialog: () => ({ openProviderSwitch: vi.fn(), providerSwitchDialog: null }) }));
vi.mock("./useTelegramThreadMenu", () => ({ useTelegramThreadMenu: () => ({ state: null, refresh: vi.fn() }) }));
vi.mock("./TabActionsMenu", () => ({ TabActionsMenu: ({ entries, onAction }: {
  entries: Array<{ kind: string; label?: string; action?: unknown }>;
  onAction: (action: never) => void;
}) => <div>{entries.filter((entry) => entry.kind === "item" && entry.action).map((entry) => <button key={entry.label} onClick={() => onAction(entry.action as never)}>{entry.label}</button>)}</div> }));
vi.mock("./ComposerContextMeter", () => ({ ComposerContextMeter: () => null }));
vi.mock("./DocumentsPanel", () => ({ DocumentsButton: () => null }));
vi.mock("./ChangesPanel", () => ({ ChangesButton: () => null }));
vi.mock("./TabModeSwitch", () => ({ TabModeSwitch: () => null }));
vi.mock("../Extensions/PluginBubbles", () => ({ PluginBubbles: () => null }));

import { TabTopBar, type HeaderTabMenu } from "./TabTopBar";
import { TabHeaderOverflow } from "./TabHeaderOverflow";
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("opens a single set of overflow controls in a dialog and dismisses it from inside or outside", async () => {
  const callbacks: ResizeObserverCallback[] = [];
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: ResizeObserverCallback) { callbacks.push(callback); }
    observe() {}
    disconnect() {}
  });
  const host = document.createElement("div");
  host.className = "ct-tab-top-bar";
  document.body.append(host);
  const root = createRoot(host);
  const action = vi.fn();
  try {
    await act(async () => root.render(<TabHeaderOverflow isActive>
      <button type="button">Usage</button>
      <button type="button" onClick={action}>Details</button>
    </TabHeaderOverflow>));
    const [usage, details] = host.querySelectorAll<HTMLButtonElement>("button");
    vi.spyOn(host, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 390, 39));
    vi.spyOn(usage.parentElement!, "getBoundingClientRect").mockReturnValue(new DOMRect(190, 6, 110, 27));
    vi.spyOn(usage, "getBoundingClientRect").mockReturnValue(new DOMRect(190, 6, 70, 27));
    vi.spyOn(details, "getBoundingClientRect").mockReturnValue(new DOMRect(190, 39, 70, 27));
    await act(async () => callbacks[0]([], {} as ResizeObserver));

    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Show all 1 hidden details"]')!;
    await act(async () => trigger.click());
    const dialog = document.querySelector<HTMLElement>('[role="dialog"][aria-label="Tab details"]')!;
    expect(dialog.contains(details)).toBe(true);
    expect(document.activeElement).toBe(dialog);
    await act(async () => details.click());
    expect(action).toHaveBeenCalledOnce();
    await act(async () => dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
    expect(dialog.isConnected).toBe(false);
    expect(host.contains(details)).toBe(true);
    expect(document.activeElement).toBe(trigger);

    await act(async () => trigger.click());
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    await act(async () => host.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })));
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(document.querySelector('[role="dialog"][aria-label="Tab details"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});

function Harness({ chat = true, locked = false, chatCapable = false, usageTabId }: { chat?: boolean; locked?: boolean; chatCapable?: boolean; usageTabId?: string }) {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  } as unknown as MultiplexerActionsContextType;
  const tab = { id: "tab-a", title: "Tab A", locked, agent_type: "fixture", agent_session_id: "session-a",
    backing: chat ? { kind: "chat_backend", plugin_id: "fixture", session_id: "session-a" } : { kind: "pty" } } as Tab;
  const tabMenu: HeaderTabMenu = {
    tab,
    context: { status: null, descriptor: null, descriptors: [], activity: null, orchestrators: [] },
  };
  return <MultiplexerActionsContext.Provider value={actions}>
    <TabTopBar tabId={usageTabId ?? "tab-a"} cwd={null} showSwitch={chatCapable} workspaceId="workspace-a" tabMenu={tabMenu} isActive
      usageVisible={Boolean(usageTabId)}
      usageSource={usageTabId ? { kind: "meter", provider: "claude", trackingProvider: "claude", borrowedAccount: false } : undefined} />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

it("reads usage on the displayed machine and drops it when that peer goes offline", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const latest = vi.fn((scope: unknown) => ({ provider: "claude", account_id: null,
    session_pct: scope === "fixture-dev" ? 31 : 73, parsed_ok: true, captured_at: Date.now() }));
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>) => (cmd === "usage_latest" ? latest(args?.peerId) : null) as T);
  const peer = (online: boolean) => ({ local_instance_name: "Local", paired: [
    { instance_id: "fixture-dev", instance_name: "Remote", online },
  ] }) as MeshPeersResponse;
  try {
    await act(async () => root.render(<Harness usageTabId="tab-a" />));
    expect(host.querySelector('button[aria-label="usage session 73%"]')).not.toBeNull();
    await act(async () => {
      updateRemoteChatPeers(peer(true));
      root.render(<Harness usageTabId="mesh:fixture-dev/tab-a" />);
    });
    expect(host.querySelector('button[aria-label="usage session 31%"]')).not.toBeNull();
    await act(async () => updateRemoteChatPeers(peer(false)));
    expect(host.querySelector('button[aria-label^="usage limits unavailable"]')).not.toBeNull();
    expect(host.querySelector('button[aria-label="usage session 31%"]')).toBeNull();
    expect(latest.mock.calls).toEqual([[null], ["fixture-dev"]]);
    await act(async () => root.render(<Harness usageTabId="tab-a" />));
    expect(host.querySelector('button[aria-label="usage session 73%"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    resetRemoteChatStatus();
    __setInvokeForTest(null);
    __setListenForTest(null);
    host.remove();
  }
});

it("shows header action failures in a retryable toast without an inline alert", async () => {
  fakes.reconnect.mockReset().mockRejectedValueOnce(new Error("opaque reconnect failure"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const reconnect = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(button => button.textContent === "Reconnect");
    await act(async () => reconnect!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const retry = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Retry");
    expect(retry).toBeDefined();
    await act(async () => retry!.click());
    expect(fakes.reconnect).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

it("keeps reconnect in the tab menu and shortcut with no separate refresh button", async () => {
  fakes.reconnect.mockReset().mockResolvedValue(undefined);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const shortcut = () => new KeyboardEvent("keydown", { code: "KeyR", ctrlKey: true, bubbles: true, cancelable: true });
  try {
    for (const props of [{}, { chat: false, chatCapable: true }]) {
      await act(async () => root.render(<Harness {...props} />));
      expect(host.querySelector('button[aria-label="Refresh chat"]')).toBeNull();
    }
    await act(async () => root.render(<Harness />));
    const menu = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(button => button.textContent === "Reconnect");
    await act(async () => menu!.click());
    const pressed = shortcut();
    await act(async () => window.dispatchEvent(pressed));
    expect(pressed.defaultPrevented).toBe(true);
    expect(fakes.reconnect.mock.calls).toEqual([["tab-a"], ["tab-a"]]);
    await act(async () => root.render(<Harness locked />));
    const locked = shortcut();
    await act(async () => window.dispatchEvent(locked));
    expect(locked.defaultPrevented).toBe(false);
    expect(fakes.reconnect).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

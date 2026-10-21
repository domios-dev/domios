import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Notification, Tab } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";

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
vi.mock("../Extensions/PluginBubbles", () => ({ PluginBubbles: () => null }));
vi.mock("../Usage/UsageBubble", () => ({ UsageBubble: () => null }));

import { TabTopBar, type HeaderTabMenu } from "./TabTopBar";
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  } as unknown as MultiplexerActionsContextType;
  const tab = { id: "tab-a", title: "Tab A", agent_type: "codex", agent_session_id: "session-a" } as Tab;
  const tabMenu: HeaderTabMenu = {
    tab,
    context: { status: null, descriptor: null, descriptors: [], activity: null, orchestrators: [] },
  };
  return <MultiplexerActionsContext.Provider value={actions}>
    <TabTopBar tabId="tab-a" cwd={null} showSwitch={false} workspaceId="workspace-a" tabMenu={tabMenu} />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

it("shows header action failures in a retryable toast without an inline alert", async () => {
  fakes.reconnect.mockReset().mockRejectedValueOnce(new Error("opaque reconnect failure"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const reconnect = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Reconnect");
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

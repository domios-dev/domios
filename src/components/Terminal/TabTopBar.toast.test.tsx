import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { MeshPeersResponse, Notification, Tab } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import { resetRemoteChatStatus, updateRemoteChatPeers } from "../../lib/remoteChatStatus";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { AccountMeshSnapshot } from "../MeshPanel/accountMeshClient";
import { rememberAccountMachineNames, rememberMachineIdentities } from "../../lib/useMeshMachineIdentities";

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
import { TabHeaderTitle } from "./TabHeaderTitle";
import { restoreStageView, transcriptLoadingStageView } from "../../lib/restoreStageView";
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const otherInstallation = {
  installation_id: "other-installation",
  name: "Other",
  active: true,
  signed_out: true,
  membership_expires_at: null,
  endpoint_expires_at: null,
  instance_id: null,
  online: false,
};
const localMesh: MeshPeersResponse = {
  local_instance_name: "Local",
  local_machine_identity: { kind: "desktop" },
  machine_identities: {},
  installations: [],
  paired: [],
  local_dev: false,
  local_last_active_ms: null,
};

it.each([
  { scenario: "a local tab on the only machine", subtitle: "Worker" },
  { scenario: "a local tab with only a revoked slot", installations: [{ ...otherInstallation, active: false }], subtitle: "Worker" },
  { scenario: "a local tab with another signed-out machine", installations: [otherInstallation], subtitle: "Worker · Local" },
  { scenario: "a local tab with a paired peer", paired: [{ instance_id: "other", instance_name: "Other" }] as MeshPeersResponse["paired"], subtitle: "Worker · Local" },
  { scenario: "a local tab with a named account peer", accountPeer: true, subtitle: "Worker · Local" },
  { scenario: "a remote tab without a matching peer", remote: true, subtitle: "Worker · Other" },
])("renders the subtitle for $scenario", async ({ installations = [], paired = [], accountPeer, remote, subtitle }) => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const tab = {
    id: remote ? "mesh:other/tab-a" : "tab-a",
    title: "Tab A",
    role: "Worker",
    remote_peer_name: remote ? "Other" : undefined,
  } as Tab;
  try {
    await act(async () => {
      rememberMachineIdentities({ ...localMesh, installations, paired });
      rememberAccountMachineNames(accountPeer ? {
        local_installation_id: "local-installation",
        registry_names: [{ installation_id: "other-installation", admitted_at: 1, display_name: "Other", active: true }],
        pairs: [{ instance_id: "other", installation_id: "other-installation", admission: "account", online: false, name: "Other" }],
      } as AccountMeshSnapshot : null);
      root.render(<TabHeaderTitle tab={tab} />);
    });
    expect(host.textContent).toBe(`Tab A${subtitle}`);
  } finally {
    await act(async () => root.unmount());
    rememberMachineIdentities({ ...localMesh, local_instance_name: "" });
    rememberAccountMachineNames(null);
  }
});

it("keeps role and machine visible and never reveals the restoring subtitle on a short restore", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const tab = {
    id: "mesh:other/tab-short-restore",
    title: "Main",
    role: "Manager",
    role_profile: "general_agent",
    agent_type: "fixture",
    agent_session_id: "session-a",
    remote_peer_name: "Other",
    frozen: true,
  } as Tab;
  try {
    await act(async () => {
      root.render(<TabHeaderTitle tab={tab} restore={restoreStageView(tab, { requested: true })} />);
    });
    const waitingStatus = host.querySelector('[role="status"]');
    expect(waitingStatus?.closest('[aria-hidden="true"]')).not.toBeNull();
    const visibleText = [...host.querySelectorAll("span")]
      .filter(el => el.children.length === 0 && !el.closest('[aria-hidden="true"]'))
      .map(el => el.textContent)
      .join("");
    expect(visibleText).toBe("MainGeneral Agent · Other");

    await act(async () => {
      root.render(<TabHeaderTitle tab={tab} restore={restoreStageView({ ...tab, frozen: false, boot_stage: "agent_ready" })} />);
    });
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(host.textContent).toBe("MainGeneral Agent · Other");
  } finally {
    await act(async () => root.unmount());
  }
});

it("announces restoring and starting stages in the header subtitle after deferral and reverts on ready, sleeping, attention, and failure", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const tab = {
    id: "tab-restore-subtitle",
    title: "Main",
    role: "Manager",
    role_profile: "general_agent",
    agent_type: "fixture",
    agent_session_id: "session-a",
    frozen: true,
  } as Tab;
  const animation = async (type: string, name: string) => act(async () => {
    const target = host.querySelector('[role="status"]')?.parentElement;
    expect(target).not.toBeNull();
    const event = new Event(type, { bubbles: true });
    Object.defineProperty(event, "animationName", { value: name });
    target!.dispatchEvent(event);
  });
  try {
    await act(async () => {
      rememberMachineIdentities({ ...localMesh, installations: [otherInstallation] });
      root.render(<TabHeaderTitle tab={tab} restore={restoreStageView(tab, { requested: true })} />);
    });
    expect(host.querySelector('[role="status"]')?.closest('[aria-hidden="true"]')).not.toBeNull();
    expect(host.textContent).toContain("General Agent · Local");

    await animation("animationstart", "ct-presence-enter");
    await animation("animationend", "ct-presence-enter");
    const restoringStatus = host.querySelector('[role="status"]');
    expect(restoringStatus?.closest('[aria-hidden="true"]')).toBeNull();
    expect(restoringStatus?.getAttribute("aria-live")).toBe("polite");
    expect(restoringStatus?.getAttribute("aria-label")).toBe("Restoring…");
    expect(restoringStatus?.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
    expect(host.textContent).toBe("MainGeneral Agent · Restoring…");

    const startingTab = { ...tab, frozen: false, boot_stage: "agent_launching" as const, agent_booting: true };
    await act(async () => {
      root.render(<TabHeaderTitle tab={startingTab} restore={transcriptLoadingStageView(startingTab)} />);
    });
    const startingStatus = host.querySelector('[role="status"]');
    expect(startingStatus?.closest('[aria-hidden="true"]')).toBeNull();
    expect(startingStatus?.getAttribute("aria-live")).toBe("polite");
    expect(startingStatus?.getAttribute("aria-label")).toBe("Starting…");
    expect(host.textContent).toBe("MainGeneral Agent · Starting…");

    for (const view of [
      restoreStageView({ ...tab, frozen: false, boot_stage: "agent_ready" }),
      restoreStageView(tab),
      restoreStageView({ ...tab, frozen: false, boot_stage: "spawn_failed", boot_failure_reason: "opaque" }),
      restoreStageView({ ...tab, frozen: false, boot_stage: "awaiting_owner_prompt" }),
    ]) {
      await act(async () => {
        root.render(<TabHeaderTitle tab={tab} restore={view} />);
      });
      if (host.querySelector('[role="status"]')) {
        expect(host.querySelector('[role="status"]')?.closest('[aria-hidden="true"]')).not.toBeNull();
        await animation("animationend", "ct-presence-exit");
      }
      expect(host.querySelector('[role="status"]')).toBeNull();
      expect(host.textContent).toBe("MainGeneral Agent · Local");
    }
  } finally {
    await act(async () => root.unmount());
    rememberMachineIdentities({ ...localMesh, local_instance_name: "" });
  }
});

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

it("omits the right-hand remote machine bubble while keeping the left-side machine identity", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const actions = { upsertToast: vi.fn() } as unknown as MultiplexerActionsContextType;
  const tab = {
    id: "mesh:peer-blackstar/tab-verdi",
    title: "Verdi",
    role: "Manager",
    role_profile: "orchestrator",
    agent_type: "fixture",
    remote_peer_id: "peer-blackstar",
    remote_peer_name: "BLACKSTAR",
  } as Tab;
  try {
    await act(async () => {
      rememberMachineIdentities({
        ...localMesh,
        paired: [{ instance_id: "peer-blackstar", instance_name: "BLACKSTAR", online: true }] as MeshPeersResponse["paired"],
      });
      root.render(
        <MultiplexerActionsContext.Provider value={actions}>
          <TabTopBar tabId={tab.id} shellTab={tab} cwd={null} showSwitch={false} isActive />
        </MultiplexerActionsContext.Provider>,
      );
    });
    expect(host.querySelector('[data-testid="remote-viewer-pill"]')).toBeNull();
    expect(host.querySelector(".ct-tab-header-title__subtitle")?.textContent).toBe("ORCHESTRATOR · BLACKSTAR");
  } finally {
    await act(async () => root.unmount());
    rememberMachineIdentities({ ...localMesh, local_instance_name: "" });
    host.remove();
  }
});

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

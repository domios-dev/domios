import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Notification, ThemeConfig } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import type { UpdateRow } from "../../lib/updatePanelModel";

const stores = vi.hoisted(() => ({
  mobile: false,
  window: { minimize: vi.fn(), toggleMaximize: vi.fn(), close: vi.fn() },
  account: {
    model: { kind: "error", message: "account-store-error" },
    signIn: vi.fn(),
    signOut: vi.fn(),
  },
  billing: {
    plan: { authority: "billing", fullAccess: false, plan: "pro", status: "unavailable", stale: false, periodEnd: null, cancelAtPeriodEnd: false },
    manage: vi.fn(async () => new Error("billing-store-error")),
  },
  update: { error: "update-store-error", busy: false, checking: false, items: [] as UpdateRow[], rows: [] as UpdateRow[],
    diagnostics: [], history: [], information: [], checkDetails: [], blockingTabs: [], retry: vi.fn() },
}));

vi.mock("../Settings/useAccount", () => ({ useAccount: () => stores.account }));
vi.mock("../../lib/nativeBridge", async (original) => {
  const module = await original<typeof import("../../lib/nativeBridge")>();
  return { ...module, nativeBridge: { ...module.nativeBridge, get window() { return stores.mobile ? undefined : stores.window; } } };
});
vi.mock("../../lib/webApi", async (original) => ({
  ...await original<typeof import("../../lib/webApi")>(), get IS_WEB() { return stores.mobile; },
}));
vi.mock("../../hooks/useSidebarViewport", () => ({ usePhoneViewport: () => stores.mobile }));
vi.mock("../../lib/keybindings", async (original) => ({
  ...await original<typeof import("../../lib/keybindings")>(), isMac: false,
}));
vi.mock("../Settings/useAccountPlan", () => ({ useAccountPlan: () => stores.billing }));
vi.mock("../billing/useDesktopBilling", () => ({ useDesktopBilling: () => ({ subscription: { paid_subscription: { tier: "pro" } } }) }));
vi.mock("../../hooks/useUpdatePanel", () => ({ useUpdatePanel: () => stores.update }));
vi.mock("../../hooks/useUpdate", () => ({ useUpdate: () => ({ phase: "idle", installing: false, downloadedBytes: 0, downloadTotalBytes: null }) }));
vi.mock("./KeepAwakeTray", () => ({ KeepAwakeTray: () => null }));
vi.mock("./ExtensionsTray", () => ({ ExtensionsTray: () => null }));
vi.mock("../WebPeerPicker/WebPeerPicker", () => ({ WebPeerPicker: () => null }));

import { Titlebar } from "./Titlebar";

Object.assign(globalThis, {
  IS_REACT_ACT_ENVIRONMENT: true,
  __CODETERM_DEV__: false,
  __APP_VERSION__: "test",
});

function Harness({ locked = false, onAnalytics, onExtensions }: {
  locked?: boolean; onAnalytics?: () => void; onExtensions?: () => void;
}) {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  } as unknown as MultiplexerActionsContextType;
  return <MultiplexerActionsContext.Provider value={actions}>
    <Titlebar
      locked={locked}
      theme={{} as ThemeConfig}
      onOpenSettings={() => {}}
      onToggleAnalytics={onAnalytics}
      onOpenExtensions={onExtensions}
      toasts={toasts}
      onDismissToast={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))}
    />
  </MultiplexerActionsContext.Provider>;
}

it("keeps an available Update operable beside the keyboard-accessible phone overflow", async () => {
  stores.mobile = true;
  stores.update.error = "";
  stores.update.rows = [{ id: "interface", group: "Interface", name: "Interface", currentVersion: "1", targetVersion: "2",
    alreadyCurrent: false, state: "available" }];
  stores.update.items = stores.update.rows;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onAnalytics = vi.fn();
  const onExtensions = vi.fn();
  try {
    await act(async () => root.render(<Harness onAnalytics={onAnalytics} onExtensions={onExtensions} />));
    const update = host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!;
    const overflow = host.querySelector<HTMLButtonElement>('button[aria-label="More actions"]')!;
    expect(update.getAttribute("aria-description")).toContain("Updates available");
    await act(async () => update.click());
    expect(update.getAttribute("aria-expanded")).toBe("true");
    expect(document.querySelector('button[aria-label="Update Interface"]')).not.toBeNull();
    await act(async () => update.click());
    overflow.focus();
    await act(async () => overflow.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
    const menu = document.querySelector('[role="menu"][aria-label="More actions"]')!;
    const analytics = [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitemcheckbox"]')]
      .find(item => item.textContent?.includes("Analytics"))!;
    expect(document.activeElement).toBe(analytics);
    expect(menu.textContent).toContain("Extensions");
    expect(menu.textContent).toContain("Settings");
    await act(async () => analytics.click());
    expect(onAnalytics).toHaveBeenCalledOnce();
    expect(update.getAttribute("aria-expanded")).toBe("false");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    stores.mobile = false;
    stores.update.error = "update-store-error";
    stores.update.rows = [];
    stores.update.items = [];
  }
});

it("omits Analytics from both titlebar entries when the shell cannot open it", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    expect(host.querySelector('button[aria-label="Analytics"]')).toBeNull();

    stores.mobile = true;
    await act(async () => root.render(<Harness />));
    const overflow = host.querySelector<HTMLButtonElement>('button[aria-label="More actions"]')!;
    overflow.focus();
    await act(async () => overflow.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
    const menu = document.querySelector('[role="menu"][aria-label="More actions"]');
    expect(menu?.textContent).toContain("Settings");
    expect(menu?.textContent).not.toContain("Analytics");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    stores.mobile = false;
  }
});

it("keeps account, billing and update errors out of the titlebar status region", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const titlebar = host.querySelector<HTMLElement>('[data-testid="titlebar"]')!;
    const toast = titlebar.querySelector<HTMLElement>('[data-testid="notification-toast"]');
    const statusRegions = Array.from(titlebar.querySelectorAll<HTMLElement>('[role="status"]'));

    expect(toast?.getAttribute("role")).toBe("status");
    expect(statusRegions).toHaveLength(1);
    expect(statusRegions.every((region) => region.closest('[data-testid="notification-toast"]'))).toBe(true);
    expect(titlebar.querySelector('[role="alert"]')).toBeNull();
    expect(titlebar.querySelector('button[aria-label="Account"]')).not.toBeNull();
    expect(titlebar.querySelector('button[aria-label="Update"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("keeps sign-in and native window controls usable while app settings stay locked before onboarding", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness locked />));
    const settings = host.querySelector<HTMLButtonElement>('[data-testid="titlebar-settings"]')!;
    const update = host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!;
    const account = host.querySelector<HTMLButtonElement>('button[aria-label="Account"]')!;
    expect(settings.disabled).toBe(true);
    expect(update.disabled).toBe(true);
    expect(account.disabled).toBe(false);
    for (const [title, action] of [
      ["Minimize", stores.window.minimize],
      ["Maximize", stores.window.toggleMaximize],
      ["Close", stores.window.close],
    ] as const) {
      const control = host.querySelector<HTMLButtonElement>(`button[title="${title}"]`)!;
      expect(control.disabled).toBe(false);
      await act(async () => control.click());
      expect(action).toHaveBeenCalledOnce();
    }
    await act(async () => account.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0 })));
    expect(document.querySelector('[role="menu"]:not([hidden])')).not.toBeNull();
    expect(Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).some((item) => !item.disabled)).toBe(true);
    await act(async () => document.querySelector<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')!.click());
    expect(stores.account.signIn).toHaveBeenCalledOnce();
    await act(async () => root.render(<Harness />));
    expect(host.querySelector('[data-testid="titlebar-settings"]')).toBe(settings);
    expect(settings.disabled).toBe(false);
    expect(update.disabled).toBe(false);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Notification, ThemeConfig } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";

const stores = vi.hoisted(() => ({
  account: {
    model: { kind: "error", message: "account-store-error" },
    signIn: vi.fn(),
    signOut: vi.fn(),
  },
  billing: {
    plan: { authority: "billing", fullAccess: false, plan: "pro", status: "unavailable", stale: false, periodEnd: null, cancelAtPeriodEnd: false },
    manage: vi.fn(async () => new Error("billing-store-error")),
  },
  update: { error: "update-store-error", busy: false, items: [], rows: [], retry: vi.fn() },
}));

vi.mock("../Settings/useAccount", () => ({ useAccount: () => stores.account }));
vi.mock("../Settings/useAccountPlan", () => ({ useAccountPlan: () => stores.billing }));
vi.mock("../billing/useDesktopBilling", () => ({ useDesktopBilling: () => ({ subscription: { paid_subscription: { tier: "pro" } } }) }));
vi.mock("../../hooks/useUpdatePanel", () => ({ useUpdatePanel: () => stores.update }));
vi.mock("./KeepAwakeTray", () => ({ KeepAwakeTray: () => null }));
vi.mock("./ExtensionsTray", () => ({ ExtensionsTray: () => null }));
vi.mock("../WebPeerPicker/WebPeerPicker", () => ({ WebPeerPicker: () => null }));

import { Titlebar } from "./Titlebar";

Object.assign(globalThis, {
  IS_REACT_ACT_ENVIRONMENT: true,
  __CODETERM_DEV__: false,
  __APP_VERSION__: "test",
});

function Harness({ locked = false }: { locked?: boolean }) {
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
      toasts={toasts}
      onDismissToast={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))}
    />
  </MultiplexerActionsContext.Provider>;
}

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

it("keeps the normal titlebar mounted with only account sign-in enabled before onboarding", async () => {
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

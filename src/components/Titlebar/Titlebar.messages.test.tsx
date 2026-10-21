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

function Harness() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  } as unknown as MultiplexerActionsContextType;
  return <MultiplexerActionsContext.Provider value={actions}>
    <Titlebar
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

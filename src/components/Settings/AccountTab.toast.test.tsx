import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Notification } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";

const fixture = vi.hoisted(() => ({
  model: { kind: "error", message: "opaque sign-in failure", failure: { stage: "redemption", error: "provider" } } as any,
  signIn: vi.fn(async () => null),
  signOut: vi.fn(async () => null),
  manage: vi.fn(async (): Promise<unknown | null> => null),
  upgrade: vi.fn(async (): Promise<unknown | null> => null),
  planStatus: "active",
}));

vi.mock("./useAccount", async (importOriginal) => ({
  ...await importOriginal<typeof import("./useAccount")>(),
  useAccount: () => ({ model: fixture.model, signIn: fixture.signIn, signOut: fixture.signOut }),
}));
vi.mock("./useAccountPlan", () => ({ useAccountPlan: () => ({
  plan: { plan: "pro", status: fixture.planStatus, stale: false, periodEnd: null, cancelAtPeriodEnd: false },
  manage: fixture.manage,
  upgrade: fixture.upgrade,
}) }));
vi.mock("../billing/PlanCards", () => ({ PlanCards: ({ onChoose }: { onChoose: (key: string) => void }) => <button onClick={() => onChoose("team")}>Choose plan</button> }));
vi.mock("../billing/useDesktopBilling", () => ({ useDesktopBilling: () => ({ catalog: null, subscription: { paid_subscription: { tier: "pro" } }, error: null, refresh: vi.fn() }) }));

import { AccountTab } from "./AccountTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  } as unknown as MultiplexerActionsContextType;
  return <MultiplexerActionsContext.Provider value={actions}>
    <AccountTab />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

afterEach(() => {
  vi.clearAllMocks();
  fixture.model = { kind: "error", message: "opaque sign-in failure", failure: { stage: "redemption", error: "provider" } };
  fixture.planStatus = "active";
});

it("shows sign-in failure details and retry in a toast, without an account pane alert", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const retry = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Retry");
    expect(retry).toBeDefined();
    expect(Array.from(host.querySelectorAll("button")).some((button) => button.textContent === "Copy details")).toBe(true);
    await act(async () => retry!.click());
    expect(fixture.signIn).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

it("does not restore a dismissed sign-in error on rerender", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Dismiss"]')!.click());
    await act(async () => root.render(<Harness />));
    expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

it("shows subscription action failures in a retryable toast", async () => {
  fixture.model = { kind: "signed_in", identity: { uid: "user", display_name: "Ada", email: "ada@example.test", provider: "fake" } };
  fixture.manage.mockResolvedValueOnce(new Error("portal_failed"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const manage = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Manage subscription");
    await act(async () => manage!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const retry = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Retry");
    await act(async () => retry!.click());
    expect(fixture.manage).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

it("keeps unavailable subscription actions informative without a retry", async () => {
  fixture.model = { kind: "signed_in", identity: { uid: "user", display_name: "Ada", email: "ada@example.test", provider: "fake" } };
  fixture.planStatus = "unavailable";
  fixture.manage.mockResolvedValueOnce(new Error("functions URL missing"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const manage = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Manage subscription");
    await act(async () => manage!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(Array.from(host.querySelectorAll("button")).some((button) => button.textContent === "Retry")).toBe(false);
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

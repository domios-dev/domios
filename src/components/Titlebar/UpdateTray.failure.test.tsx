import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Notification } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";

const state = vi.hoisted(() => ({
  error: "opaque failure" as string | null,
  busy: false,
  items: [] as unknown[],
  rows: [] as unknown[],
  retry: vi.fn(),
}));
vi.mock("../../hooks/useUpdatePanel", () => ({ useUpdatePanel: () => state }));
vi.mock("../UpdatePanel", () => ({ UpdatePanel: () => <div>fixture details</div> }));
import { UpdateTray } from "./UpdateTray";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function Harness() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  } as unknown as MultiplexerActionsContextType;
  return <MultiplexerActionsContext.Provider value={actions}>
    <UpdateTray />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

it("reports update failures in a retryable toast without titlebar error text", async () => {
  state.error = "opaque failure";
  state.retry.mockClear();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const render = () => act(async () => root.render(<Harness />));
  const trigger = () => host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!;
  try {
    await render();
    expect(trigger().textContent).toContain("Updates");
    expect(trigger().textContent).not.toContain("failed");
    expect(trigger().title).not.toContain("failed");
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const retry = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Retry");
    await act(async () => retry!.click());
    expect(state.retry).toHaveBeenCalledOnce();
    await act(async () => trigger().click());
    expect(document.querySelector('[role="dialog"][aria-label="Updates"]')).not.toBeNull();
    await act(async () => trigger().click());
    expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
    state.error = null;
    await render();
    expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
    state.error = "second failure";
    await render();
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

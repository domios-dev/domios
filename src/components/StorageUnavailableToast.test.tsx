import { act, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import type { Notification } from "../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../store/multiplexer/context";
import { NotificationToastContainer } from "./Notifications/NotificationToast";
import { StorageUnavailableToast } from "./StorageUnavailableToast";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const [active, setActive] = useState(true);
  const [revision, setRevision] = useState(0);
  const actions = useMemo(() => ({
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
    dismissToast: (id: string) => setToasts((current) => current.filter((toast) => toast.id !== id)),
  }), []);
  return <MultiplexerActionsContext.Provider value={actions as unknown as MultiplexerActionsContextType}>
    <StorageUnavailableToast active={active} />
    <button onClick={() => setRevision(revision + 1)}>Rerender</button>
    <button onClick={() => setActive(false)}>Clear error</button>
    <NotificationToastContainer toasts={toasts} onDismiss={actions.dismissToast} />
  </MultiplexerActionsContext.Provider>;
}

it("shows storage failure as a toast and keeps a dismissed notice cleared on rerender", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Dismiss"]')!.click());
    await act(async () => Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Rerender")!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
    await act(async () => Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Clear error")!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
  } finally {
    await act(async () => root.unmount()); host.remove();
  }
});

import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { connectNoticeToasts } from "../../lib/notices";
import type { Notification } from "../../lib/types";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import { useSurfaceNotices } from "./useSurfaceNotices";
import type { SurfaceNotice } from "./surfaceStreamTransport";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness({ notice, need }: { notice: SurfaceNotice | null; need: string | null }) {
  const [toasts, setToasts] = useState<Notification[]>([]);
  useEffect(() => connectNoticeToasts((toast) => setToasts((current) => [...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false }])), []);
  useSurfaceNotices(notice, need, "surface");
  return <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((item) => item.id !== id))} />;
}

it("renders browser notices and attention requests in the shared stack with dismissal", async () => {
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  const notice: SurfaceNotice = { text: "opaque browser result", tone: "error" };
  try {
    await act(async () => root.render(<Harness notice={notice} need="opaque attention" />));
    expect(host.querySelectorAll('[data-testid="notification-toast"]')).toHaveLength(2);
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Show notification: Browser notice"]')!.click());
    expect(document.querySelector('[role="dialog"] pre')?.textContent).toBe(notice.text);
    await act(async () => document.querySelector<HTMLButtonElement>('button[aria-label="Close expanded notification"]')!.click());
    expect(host.querySelector('[data-testid="surface-notice"]')).toBeNull();
    await act(async () => { for (const button of host.querySelectorAll<HTMLButtonElement>('button[aria-label="Dismiss"]')) button.click(); });
    await act(async () => root.render(<Harness notice={notice} need="opaque attention" />));
    expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

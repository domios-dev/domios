import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Notification } from "../../lib/types";
import { NotificationBell } from "./NotificationBell";

vi.mock("@codeterm/chat-ui", () => ({ RichMarkdown: ({ text }: { text: string }) => <div>{text}</div> }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("opens full details and sends the offered choice once through the bell answer handler", async () => {
  const notification: Notification = {
    id: "ask-token", tab_id: "asking-tab", workspace_id: null, title: "question-token", body: "details-token",
    level: "action_needed", severity: "Info", read: false, timestamp: 1,
    actions: [{ type: "answer", label: "choice-token" }],
    answer: { asked_from_tab: "asking-tab", answered: null, delivered: false },
  };
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const answer = vi.fn();
  try {
    await act(async () => root.render(<NotificationBell notifications={[notification]}
      onMarkRead={vi.fn()} onMarkAllRead={vi.fn()} onDelete={vi.fn()} onClearAll={vi.fn()}
      onNavigate={vi.fn()} onAnswer={answer} />));
    expect(host.querySelector('[role="status"]')?.textContent).toContain("Unread notifications");
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Notifications"]')!.click());
    const details = document.querySelector('[role="region"][aria-label="Message from question-token"]');
    expect(details?.textContent).toContain(notification.body);
    const choice = [...document.querySelectorAll("button")].find((button) => button.textContent === "choice-token");
    expect(choice).toBeDefined();
    await act(async () => choice!.click());
    expect(answer).toHaveBeenCalledExactlyOnceWith(notification.id, "choice-token");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

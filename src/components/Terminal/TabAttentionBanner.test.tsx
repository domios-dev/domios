import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Notification, Tab } from "../../lib/types";
import { TabAttentionBanner } from "./TabAttentionBanner";
import { useTabAttention } from "./useTabAttention";

const tab: Tab = { id: "agent", title: "tab-token", pty_id: null, tab_type: "Terminal", cwd: null };
const notification: Notification = {
  id: "n", tab_id: tab.id, workspace_id: "ws", title: "reason-token", body: "body-token",
  level: "action_needed", severity: "Info", read: false, timestamp: 1,
  actions: [{ type: "answer", label: "choice-token" }],
  answer: { asked_from_tab: tab.id, answered: null, delivered: false },
};
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
const answer = vi.fn();
const markRead = vi.fn();
function Harness({ notifications }: { notifications: Notification[] }) {
  const { model, dismiss } = useTabAttention(tab, null, notifications, true, markRead);
  return <TabAttentionBanner title={tab.title} model={model} onDismiss={dismiss} onAction={answer} />;
}
async function render(notifications: Notification[]) {
  await act(async () => root.render(<Harness notifications={notifications} />));
}
function button(name: string): HTMLButtonElement {
  const found = [...host.querySelectorAll("button")].find((element) =>
    (element.getAttribute("aria-label") ?? element.textContent) === name);
  expect(found).toBeDefined();
  return found!;
}
afterEach(async () => {
  await act(async () => root.render(null));
  answer.mockClear();
  markRead.mockClear();
});

it("wires expansion and choice to accessible controls", async () => {
  await render([notification]);
  expect(host.textContent).not.toContain(notification.body);
  await act(async () => button(`Choose answer for ${notification.title}`).click());
  expect(host.textContent).toContain(notification.body);
  await act(async () => button("choice-token").click());
  expect(answer).toHaveBeenCalledExactlyOnceWith(notification.actions![0]);
  await act(async () => button(`Hide attention details for ${tab.title}`).click());
  expect(host.textContent).not.toContain(notification.body);
});

it("drops the visited surface on answer and retains the notification record", async () => {
  await render([notification]);
  expect(host.querySelector('[role="region"]')).not.toBeNull();
  const answered = { ...notification, read: true, answer: { ...notification.answer!, answered: "choice-token" } };
  await render([answered]);
  expect(host.querySelector('[role="region"]')).toBeNull();
  expect(answered.body).toBe(notification.body);
});

it("drops the visited surface on clear or resolution", async () => {
  await render([notification]);
  await render([]);
  expect(host.querySelector('[role="region"]')).toBeNull();
  await render([notification]);
  await render([{ ...notification, resolved: true }]);
  expect(host.querySelector('[role="region"]')).toBeNull();
});

it("keeps integration notices without choices behind the details control", async () => {
  const details = ["body-token", "state-token", "next-step-token", `/install/${"path-token/".repeat(80)}`];
  const integrationNotice: Notification = {
    ...notification,
    id: "integration-notice",
    title: "provider-token status-token",
    body: details.join("\n\n"),
    actions: undefined,
    answer: undefined,
  };
  await render([integrationNotice]);
  await render([{ ...integrationNotice, read: true }]);
  expect(host.querySelector('[role="region"]')?.getAttribute("aria-label"))
    .toContain(integrationNotice.title);
  for (const detail of details) expect(host.textContent).not.toContain(detail);
  const expand = button(`Show attention details for ${tab.title}`);
  expect(expand.getAttribute("aria-expanded")).toBe("false");
  await act(async () => expand.click());
  for (const detail of details) expect(host.textContent).toContain(detail);
  const collapse = button(`Hide attention details for ${tab.title}`);
  expect(collapse.getAttribute("aria-expanded")).toBe("true");
  await act(async () => collapse.click());
  for (const detail of details) expect(host.textContent).not.toContain(detail);
  await act(async () => button(`Dismiss attention for ${tab.title}`).click());
  expect(host.querySelector('[role="region"]')).toBeNull();
});

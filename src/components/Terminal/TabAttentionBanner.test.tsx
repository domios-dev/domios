import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { Window } from "happy-dom";
import type { Notification, Tab } from "../../lib/types";
import { TabItem } from "../Sidebar/TabItem";
import { SidebarCompactRail } from "../Sidebar/SidebarCompactRail";
import { tabAttention, tabAttentionLabel } from "../Sidebar/tabAttention";
import { TabAttentionBanner } from "./TabAttentionBanner";
import { useTabAttention } from "./useTabAttention";

let checked = 0;
function assert(value: unknown, message: string): asserts value {
  checked++;
  if (!value) throw new Error(message);
}
const window = new Window({ url: "http://localhost/tab-attention" });
for (const key of ["window", "document", "navigator", "HTMLElement", "SVGElement", "Node", "CustomEvent"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : window[key], configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const tab: Tab = { id: "agent", title: "tab-token", agent_type: "test-provider", pty_id: null, tab_type: "Terminal", cwd: null };
const notification: Notification = { id: "n", tab_id: tab.id, workspace_id: "ws", title: "reason-token", body: "body-token", level: "failure", severity: "Info", read: false, timestamp: 1, actions: [{ type: "answer", label: "choice-token" }] };
const reads: string[] = [];
const readCount = (): number => reads.length;
const answers: string[] = [];
const markRead = (id: string) => { reads.push(id); };
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
function Harness({ active, notifications }: { active: boolean; notifications: Notification[] }) {
  const { model, dismiss } = useTabAttention(tab, null, notifications, active, markRead);
  return <>
    <TabItem tab={tab} index={0} isActive={active} onClick={() => root.render(<StrictMode><Harness active notifications={notifications} /></StrictMode>)} onClose={() => {}} canClose={false} workspaceId="ws" attention={tabAttention(tab, null, notifications, active)} />
    <TabAttentionBanner title={tab.title} model={model} onDismiss={dismiss} onAction={(action) => answers.push(action.label)} />
  </>;
}
async function render(active: boolean, notifications: Notification[]) {
  await act(async () => root.render(<StrictMode><Harness active={active} notifications={notifications} /></StrictMode>));
}
async function run() {
  const attention = tabAttention(tab, null, [notification]);
  const label = tabAttentionLabel(tab.title, attention);
  const row = renderToStaticMarkup(<TabItem tab={tab} index={0} isActive={false} onClick={() => {}} onClose={() => {}} canClose={false} workspaceId="ws" attention={attention} />);
  assert(row.includes(`role="img" aria-label="${label}"`), "row renders the model's accessible mark");
  const rail = renderToStaticMarkup(<SidebarCompactRail groups={[{ workspaceId: "ws", label: "group-token", tabs: [tab], activeTabId: null }]} tabStatuses={{}} attentionByTab={{ [tab.id]: attention }} onActivate={() => {}} />);
  assert(rail.includes(`role="img" aria-label="${label}"`), "rail renders the same accessible mark");
  const notices = [notification, { ...notification, id: "action", level: "action_needed" as const }];
  await render(false, notices);
  assert(readCount() === 0 && !host.querySelector('[role="region"]'), "unvisited tabs neither clear nor show a banner");
  const open = host.querySelector<HTMLButtonElement>(`button[aria-label="Open tab ${tab.title}"]`);
  assert(open, "tab has an accessible activation control");
  await act(async () => open.click());
  assert(reads.join() === "n,action", "opening marks each attention record read exactly once");
  const region = host.querySelector('[role="region"]');
  assert(region?.getAttribute("aria-label") === label, "banner exposes the model's accessible name");
  assert(region.textContent?.includes(notification.title) && region.textContent.includes(notification.body!), "banner renders the supplied title and body");
  await render(true, notices.map((item) => ({ ...item, read: true })));
  assert(readCount() === 2, "read projection does not repeat mark-read");
  assert(host.querySelector('[role="region"]')?.textContent?.includes(notification.title), "reason remains visible after the read update");
  const choice = [...host.querySelectorAll("button")].find((button) => button.textContent === notification.actions![0].label);
  assert(choice, "existing choice is available inline");
  await act(async () => choice.click());
  assert(answers.join() === notification.actions![0].label, "choice calls the existing action callback");
  const dismiss = host.querySelector<HTMLButtonElement>(`button[aria-label="Dismiss attention for ${tab.title}"]`);
  assert(dismiss, "banner has an accessible dismiss action");
  await act(async () => dismiss.click());
  assert(!host.querySelector('[role="region"]'), "dismiss removes the banner");
  await act(async () => root.unmount());
  console.log(`TabAttentionBanner wiring: ${checked} assertions passed`);
}
void run();

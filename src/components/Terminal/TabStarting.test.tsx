import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import { Window } from "happy-dom";
import type { Tab } from "../../lib/types";
import { tabActivity } from "../../lib/tabActivity";
import { TabItem } from "../Sidebar/TabItem";
import { tabAttention, tabStatusMark } from "../Sidebar/tabAttention";
import { ChatSetupState, chatSetupView } from "./ChatSetupState";
import { TabBootOverlay, tabBootOverlayView } from "./bootOverlay";
import { AgentStateIndicator } from "./AgentStateIndicator";
import { SidebarCompactRail } from "../Sidebar/SidebarCompactRail";

const window = new Window();
const tab: Tab = { id: "tab-token", title: "title-token", agent_type: "provider-token", tab_type: "Terminal", pty_id: null, cwd: null, boot_stage: "agent_launching", agent_state: { state: "working" } };
function parsed(element: React.ReactNode) {
  const host = window.document.createElement("div");
  host.innerHTML = renderToStaticMarkup(element);
  return host;
}
function assertLoading(host: ReturnType<typeof parsed>, label?: string, role = "status") {
  const mark = host.querySelector('[data-testid="tab-loading-mark"]');
  assert.ok(mark);
  assert.equal(mark.getAttribute("role"), role);
  assert.ok(mark.getAttribute("aria-label"));
  if (label) assert.equal(mark.getAttribute("aria-label"), label);
  assert.equal(host.querySelector('[data-activity-state]'), null);
}
for (const boot_stage of ["agent_launching", "launch_binding_unconfirmed"] as const) {
  const starting = { ...tab, boot_stage };
  const attention = tabAttention(starting, "Working", []);
  const mark = tabStatusMark(tab.title, attention)!;
  const row = parsed(<TabItem tab={starting} status="Working" hasAgent index={0} isActive={false} onClick={() => {}} onClose={() => {}} canClose={false} workspaceId="ws" />);
  assertLoading(row, mark.label, "img");
  assert.equal(row.querySelector('[data-testid="tab-working-activity"]'), null);
  const rail = parsed(<SidebarCompactRail groups={[{ workspaceId: "ws", label: "group-token", tabs: [starting], activeTabId: null }]} tabStatuses={{ [tab.id]: "Working" }} attentionByTab={{ [tab.id]: attention }} onActivate={() => {}} />);
  assertLoading(rail, mark.label, "img");
  const chat = parsed(<ChatSetupState tab={starting} view={chatSetupView(starting)} providerName="provider-token" />);
  assert.ok(chat.querySelector('[role="progressbar"][aria-valuetext]'));
  assert.ok(chat.querySelector('[role="status"][aria-label]'));
  assertLoading(parsed(<TabBootOverlay view={tabBootOverlayView(starting)} shellReady={false} overlayVisible onRetry={() => {}} onStartFreshAgent={() => {}} onStartNewSession={() => {}} onKeepShell={() => {}} />), mark.label);
  assertLoading(parsed(<AgentStateIndicator tab={starting} activity={tabActivity(starting, "Working")} />), mark.label);
}
assertLoading(parsed(<TabBootOverlay view={{ kind: "none" }} shellReady={false} overlayVisible onRetry={() => {}} onStartFreshAgent={() => {}} onStartNewSession={() => {}} onKeepShell={() => {}} />));
const working: Tab = { ...tab, boot_stage: "agent_ready", agent_session_id: "bound-token" };
assert.ok(parsed(<TabItem tab={working} status="Working" hasAgent index={0} isActive={false} onClick={() => {}} onClose={() => {}} canClose={false} workspaceId="ws" />).querySelector('[data-testid="tab-working-activity"]'));
assert.ok(parsed(<AgentStateIndicator activity={tabActivity(working, "Working")} />).querySelector('[data-testid="agent-activity-dots"]'));
window.close();
console.log("TabStarting wiring: sidebar, rail, chat setup, terminal overlay and chat status load calmly; bound work shows activity dots");

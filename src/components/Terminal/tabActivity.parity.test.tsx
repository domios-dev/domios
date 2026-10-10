import { harnessWindow } from "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { TabItem } from "../Sidebar/TabItem";
import { TabSessionChat } from "./TabSessionChat";
import { MultiplexerActionsContext, type MultiplexerActionsContextType, MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import type { Tab, TabStatus } from "../../lib/types";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";

delete (harnessWindow as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;

const agent: Tab = { id: "grok", title: "Grok", agent_type: "grok", pty_id: "pty", tab_type: "Terminal", cwd: null, agent_session_id: "own-session", session_binding_nonce: "own-nonce" };
let state: ExtendedState = {
  workspaces: [], tabs: { grok: agent }, teams: [], active_workspace_id: null, focused_tab_id: null,
  notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
};
const store = createMuxStateStore(() => ({ ...state, tabs: { grok: { ...state.tabs.grok } } }));
let stopped = 0;
__setInvokeForTest(async (command) => {
  if (command === "interrupt_agent") stopped++;
  if (command === "chat_start") return {
    source_session_id: "own-session", session_binding_nonce: "own-nonce",
    subscription_id: "activity-subscription", messages: [], offset: 0, text: "",
  } as never;
  if (command === "delayed_send_list" || command === "chat_send_status") return [] as never;
  return {} as never;
});
__setListenForTest(async () => () => {});
const noop = () => {};
const actions = {
  setFocusedTab: noop, selectTabByTab: async () => {}, addNotification: noop, answerNotification: noop,
} as unknown as MultiplexerActionsContextType;
const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);

async function main() {
  try {
    const cases: Array<[Tab, TabStatus | null, boolean, boolean]> = [
      [agent, "Working", false, false],
      [{ ...agent, agent_state: { state: "idle" } }, "Working", false, false],
      [{ ...agent, agent_state: { state: "working" } }, "Idle", true, true],
      [{ ...agent, agent_state: { state: "question", subtype: "permission" } }, "Waiting", false, false],
      [{ ...agent, boot_stage: "agent_launching" }, "Working", false, false],
      [{ ...agent, boot_stage: "agent_ready", agent_state: { state: "idle" } }, "Idle", false, false],
    ];
    for (const [tab, status, active, canStop] of cases) {
      state = { ...state, tabs: { grok: tab }, tabStatuses: status ? { grok: status } : {} };
      await act(async () => {
        root.render(<MuxStateStoreContext.Provider value={store}>
          <TabItem tab={tab} status={status} hasAgent index={0} isActive={false} onClick={() => {}} onClose={() => {}} canClose workspaceId="ws" />
          <MultiplexerActionsContext.Provider value={actions}><TabSessionChat tabId="grok" /></MultiplexerActionsContext.Provider>
        </MuxStateStoreContext.Provider>);
        store.notify();
      });
      const sidebar = host.querySelector('[data-testid="tab-working-activity"]');
      const chat = host.querySelector('[data-testid="agent-activity-dots"]');
      assert.equal(Boolean(sidebar), active, "sidebar shows dots only for a working turn");
      assert.equal(Boolean(chat), active, "chat agrees with sidebar about actual work");
      assert.equal(
        host.querySelector('[data-testid="tab-item"]')?.getAttribute("data-tab-activity"),
        host.querySelector('[data-testid="agent-state"]')?.getAttribute("data-tab-activity"),
        "sidebar and real chat row render the same activity kind",
      );
      const stop = host.querySelector<HTMLButtonElement>('button[aria-label="Stop the agent"]');
      assert.equal(Boolean(stop && !stop.disabled), canStop, "Stop follows activity, including non-stoppable launch");
      if (canStop && stopped === 0) await act(async () => stop!.click());
    }
    assert.equal(stopped, 1, "Stop reaches the actual interrupt command exactly once");
    console.log("sidebar and chat activity parity: working, idle, waiting, starting passed");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
    await harnessWindow.happyDOM.abort();
    harnessWindow.close();
  }
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });

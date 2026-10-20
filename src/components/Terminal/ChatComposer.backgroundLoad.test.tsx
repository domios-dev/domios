import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import type { Tab } from "../../lib/types";
import { ChatComposer } from "./ChatComposer";
import type { InitializeCapabilities } from "../../lib/providerCapabilities";

test("unchanged background snapshots do not render the composer and local edits survive changed commands", async () => {
  const tabId = "background-load";
  const data: InitializeCapabilities = {
    commands: [{ name: "fixture", description: "opaque", argument_hint: null }],
    models: null, agents: null, account: null, current_permission_mode: null,
    fast_mode_state: null, output_style: null,
  };
  let state: ExtendedState = {
    workspaces: [], tabs: { [tabId]: { id: tabId, provider_signals: { capabilities: { status: "ready", data } } } as Tab },
    teams: [], active_workspace_id: null, focused_tab_id: null, notifications: [],
    tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const store = createMuxStateStore(() => state);
  const calls: string[] = [];
  let finishDelayed!: (value: unknown) => void;
  const delayed = new Promise<unknown>(resolve => { finishDelayed = resolve; });
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    calls.push(command);
    if (command === "delayed_send_list") return await delayed as T;
    if (["list_agent_providers", "cron_list"].includes(command)) return [] as T;
    if (command === "plugin_tab_menu") return { items: [], receiverTargets: {} } as T;
    throw new Error(`Unexpected command: ${command}`);
  });
  __setListenForTest(async () => () => {});
  window.__composerPerf = { inputMarks: [], paintLatencies: [], renderCounts: {}, compositionEvents: [], ignoredKeys: [] };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const reject = () => { throw new Error("Background updates must not send or mutate"); };
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={store}>
      <ChatComposer tabId={tabId} tabProvider={null} currentPtyId={null} pendingPrompt={null}
        attachments={[]} addAttachments={reject} removeAttachment={reject} clearAttachments={reject}
        handlePasteEvent={reject} onWillSendMessage={reject} addOptimistic={reject}
        updateOptimistic={reject} observeChatSendStatus={reject} showComposerError={reject} />
    </MuxStateStoreContext.Provider>));
    const editor = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]');
    assert(editor);
    const initialRenders = window.__composerPerf.renderCounts[tabId];
    const initialCalls = calls.length;
    for (let generation = 2; generation <= 11; generation++) {
      await act(async () => {
        state = { ...structuredClone(state), generation };
        store.notify();
      });
    }
    assert.equal(window.__composerPerf.renderCounts[tabId], initialRenders, "identical capabilities must not cause editor work");
    const setValue = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")!.set!;
    await act(async () => {
      setValue.call(editor, "/fi");
      editor.dispatchEvent(new window.Event("input", { bubbles: true }));
    });
    assert.equal(editor.value, "/fi");
    assert(document.querySelector('[role="listbox"][aria-label="Command suggestions"]'));
    assert(document.querySelector('[role="option"][data-slash-command="/fixture"]'));
    const editedRenders = window.__composerPerf.renderCounts[tabId];
    await act(async () => {
      state = structuredClone(state);
      state.tabs[tabId].provider_signals!.capabilities = { status: "ready", data: { ...data, account: { subscription_type: "changed", api_provider: null } } };
      store.notify();
    });
    assert.equal(window.__composerPerf.renderCounts[tabId], editedRenders, "unrelated capability changes do not render the editor");
    assert.equal(editor.value, "/fi", "typing stays local while the daemon read is pending");
    await act(async () => {
      state = structuredClone(state);
      state.tabs[tabId].provider_signals!.capabilities = { status: "ready", data: { ...data, commands: [{ name: "final", description: null, argument_hint: null }] } };
      store.notify();
    });
    assert.equal(editor.value, "/fi", "authoritative command changes preserve the immediate local edit");
    assert(document.querySelector('[role="option"][data-slash-command="/final"]'));
    assert.equal(host.querySelector('[data-slash-command="/fixture"]'), null);
    assert.equal(calls.length, initialCalls, "background state and typing require no daemon commands");
  } finally {
    finishDelayed([]);
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
    delete window.__composerPerf;
  }
});

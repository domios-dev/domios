import "./ChatComposer.composition.harness";
import { strict as assert } from "node:assert";
import { test } from "node:test";
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";
import { DraftStorageFixture } from "../../test/draftStorage.testdata";
import { useProviderPromptLifecycle } from "./useProviderPromptLifecycle";
import { AgentStateIndicator } from "./AgentStateIndicator";
import { manualPendingPrompt } from "../../lib/promptAnswers";
import { tabActivity } from "../../lib/tabActivity";
import type { Tab } from "../../lib/types";

async function withComposer(
  attachments: string[],
  check: (host: HTMLDivElement, removed: string[], update: (tab: Tab) => void) => void | Promise<void>,
  initialTab?: Tab,
) {
  const unexpected: string[] = [];
  const removed: string[] = [];
  const reject = () => { throw new Error("Unexpected composer action"); };
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "list_agent_providers" || command === "delayed_send_list" || command === "system_heartbeat_list" || command === "cron_list") return [] as T;
    if (command === "plugin_tab_menu") return { items: [], receiverTargets: {} } as T;
    unexpected.push(command);
    throw new Error(`Unexpected bridge command: ${command}`);
  });
  __setListenForTest(async () => () => {});
  const state: ExtendedState = {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
  };
  const store = createMuxStateStore(() => state);
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let updateTab: (tab: Tab) => void = () => { throw new Error("Harness not mounted"); };
  function Presentation() {
    const [tab, setTab] = useState(initialTab);
    updateTab = setTab;
    const lifecycle = useProviderPromptLifecycle(tab);
    return <>
      {tab && <AgentStateIndicator tab={lifecycle.tab} activity={tabActivity(lifecycle.tab)}
        onOpenTerminal={reject} onCancel={reject} />}
      <ChatComposer
            tabId="presentation-only"
            tabProvider={null}
            currentPtyId={null}
            pendingPrompt={manualPendingPrompt(lifecycle.tab)}
            attachments={attachments}
            addAttachments={reject}
            removeAttachment={(path) => removed.push(path)}
            clearAttachments={reject}
            handlePasteEvent={reject}
            onWillSendMessage={reject}
            addOptimistic={reject}
            updateOptimistic={reject}
            observeChatSendStatus={reject}
            showComposerError={reject}
          />
    </>;
  }
  try {
    await act(async () => {
      root.render(
        <DraftStorageFixture><MuxStateStoreContext.Provider value={store}>
          <Presentation />
        </MuxStateStoreContext.Provider></DraftStorageFixture>,
      );
    });
    await act(async () => { await new Promise<void>((resolve) => setImmediate(resolve)); });
    await check(host, removed, tab => updateTab(tab));
    assert.deepEqual(unexpected, [], "presentation must not issue transport or mutation commands");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
}

test("auto-confirm clears the terminal question controls and composer waiting hint immediately", async () => {
  const pending = { id: "presentation-only", agent_type: "opaque-provider", agent_session_id: "session",
    session_binding_nonce: "launch", pty_id: "pty", pending_prompt_kind: "provider",
    agent_state: { state: "question" }, pending_prompt: { request_id: "request", question: "opaque-question",
      options: [{ label: "opaque-choice", value: "1" }] } } as Tab;
  await withComposer([], async (host, _removed, update) => {
    const editor = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]')!;
    const waiting = editor.placeholder;
    assert(host.querySelector('button[aria-label="Cancel the terminal question"]'));
    assert([...host.querySelectorAll("button")].some(button => button.textContent?.includes("Open terminal")));
    await act(async () => update({ ...pending, permission_auto_confirmations: [{ session_id: "session",
      prompt_fingerprint: "fingerprint", tool_name: "opaque-tool", question: "opaque-question", options: ["opaque-choice"],
      option_label: "opaque-choice", rule: "opaque-rule", confirmed_at_ms: 1 }] }));
    assert.notEqual(editor.placeholder, waiting);
    assert.equal(host.querySelector('button[aria-label="Cancel the terminal question"]'), null);
    assert(![...host.querySelectorAll("button")].some(button => button.textContent?.includes("Open terminal")));
    assert(!host.querySelector('[role="status"]')?.textContent?.includes("opaque-question"));
  }, pending);
});

test("composer keeps one editor in an accessible card with a separate side action", async () => {
  await withComposer([], (host) => {
    const card = host.querySelector('[role="group"][aria-label="Message composer"]')!;
    const editor = host.querySelector('[data-testid="chat-composer"]')!;
    const action = host.querySelector('[data-testid="composer-voice-button"]')!;
    assert(card.contains(editor));
    assert(!card.contains(action));
    assert.equal(host.querySelectorAll("textarea").length, 1);
  });
});

test("attachments remain accessible without composer model controls", async () => {
  await withComposer(["/fixture/one.png", "/fixture/two.jpg"], (host) => {
    const card = host.querySelector('[role="group"][aria-label="Message composer"]')!;
    assert.equal(card.querySelectorAll('[data-testid="chat-inline-attachment"]').length, 2);
    assert.equal(card.querySelector('[aria-label="Model and traits"]'), null);
  });
});

test("the clock opens scheduling for the current long draft without sending it", async () => {
  await withComposer([], async (host) => {
    const editor = host.querySelector<HTMLTextAreaElement>('textarea')!;
    const draft = "A long draft\n".repeat(80);
    await act(async () => {
      Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")!.set!.call(editor, draft);
      editor.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const clock = host.querySelector<HTMLButtonElement>('button[aria-label="Schedule send"]');
    assert(clock, "the scheduling control remains available with long text");
    await act(async () => clock.click());
    const dialog = document.querySelector('[role="dialog"]');
    assert(dialog, "the clock opens the scheduling dialog");
    assert.equal(dialog.querySelector('textarea')?.value, draft.trim());
    assert.equal(editor.value, draft, "opening scheduling preserves the draft");
  });
});

test("attachments retain accessible removal and the exact original path", async () => {
  const path = "C:\\fixture\\Screenshot.PNG";
  await withComposer([path], async (host, removed) => {
    const remove = host.querySelector<HTMLButtonElement>('[aria-label="Remove Screenshot.PNG"]')!;
    assert.equal(remove.getAttribute("aria-label"), "Remove Screenshot.PNG");
    await act(async () => remove.click());
    assert.deepEqual(removed, [path]);
  });
});

test("long nonimage filenames and four-visible overflow remain usable", async () => {
  const paths = Array.from({ length: 6 }, (_, i) => `/fixture/${"long-name-".repeat(20)}${i}.txt`);
  await withComposer(paths, (host) => {
    const chips = host.querySelectorAll<HTMLElement>('[data-testid="attachment-chip"]');
    assert.equal(chips.length, 4);
    assert.equal(host.querySelector('[data-testid="attachment-overflow-bubble"]')?.textContent, "and 2 more");
    for (const [i, chip] of [...chips].entries()) {
      assert.equal(chip.querySelector("span")?.getAttribute("title"), paths[i]);
    }
  });
});

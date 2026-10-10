import "./ChatComposer.composition.harness";
import { strict as assert } from "node:assert";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";

async function withComposer(
  attachments: string[],
  check: (host: HTMLDivElement, removed: string[]) => void | Promise<void>,
) {
  const unexpected: string[] = [];
  const removed: string[] = [];
  const reject = () => { throw new Error("Unexpected composer action"); };
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "list_agent_providers" || command === "delayed_send_list" || command === "cron_list") return [] as T;
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
  try {
    await act(async () => {
      root.render(
        <MuxStateStoreContext.Provider value={store}>
          <ChatComposer
            tabId="presentation-only"
            tabProvider={null}
            currentPtyId={null}
            pendingPrompt={null}
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
        </MuxStateStoreContext.Provider>,
      );
    });
    await act(async () => { await new Promise<void>((resolve) => setImmediate(resolve)); });
    await check(host, removed);
    assert.deepEqual(unexpected, [], "presentation must not issue transport or mutation commands");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
}

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
    assert.equal(card.querySelectorAll('[data-testid="attachment-chip"]').length, 2);
    assert.equal(card.querySelector('[aria-label="Model and traits"]'), null);
  });
});

test("attachments retain accessible removal and the exact original path", async () => {
  const path = "C:\\fixture\\Screenshot.PNG";
  await withComposer([path], async (host, removed) => {
    const chip = host.querySelector<HTMLElement>('[data-testid="attachment-chip"]')!;
    assert.equal(chip.querySelector("span")?.getAttribute("title"), path);
    const remove = chip.querySelector<HTMLButtonElement>("button")!;
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

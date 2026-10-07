import "./ChatComposer.composition.harness";
import { strict as assert } from "node:assert";
import { test } from "node:test";
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";

async function withComposer(
  attachments: string[],
  check: (host: HTMLDivElement, removed: string[]) => void | Promise<void>,
  modelBubble?: ReactNode,
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
            modelBubble={modelBubble}
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

test("composer limits content width inside the full-width divider", async () => {
  await withComposer([], (host) => {
    const shell = host.querySelector<HTMLElement>('[data-testid="chat-composer-shell"]')!;
    const row = host.querySelector<HTMLElement>('[data-testid="composer-row"]')!;
    assert(shell.classList.contains("border-t"));
    assert(!shell.classList.contains("max-w-4xl"));
    assert(row.parentElement?.classList.contains("max-w-4xl"), "input must share the conversation width bound");
    assert(row.parentElement?.classList.contains("mx-auto"));
    assert.equal(row.parentElement?.parentElement, shell);
    assert.equal(host.querySelector('[data-testid="attachment-chip"]'), null);
  });
});

test("attachments participate individually in the shared wrapping row", async () => {
  await withComposer(["/fixture/one.png", "/fixture/two.jpg"], (host) => {
    const bubbles = host.querySelector<HTMLElement>('[data-testid="composer-bubble-row"]')!;
    assert(bubbles.classList.contains("contents"), "attachment group must not force a separate line");
    assert(bubbles.parentElement?.classList.contains("flex-wrap"));
    assert.equal(bubbles.firstElementChild?.getAttribute("data-testid"), "attachment-chip");
    assert.equal(bubbles.lastElementChild?.getAttribute("data-testid"), "supplied-model");
    assert(!bubbles.className.includes("max-h-"));
  }, <button data-testid="supplied-model">Fixture model</button>);
});

test("image capsules retain accessible removal and the exact original path", async () => {
  const path = "C:\\fixture\\Screenshot.PNG";
  await withComposer([path], async (host, removed) => {
    const chip = host.querySelector<HTMLElement>('[data-testid="attachment-chip"]')!;
    assert(chip.classList.contains("rounded-full"), "image attachment must be a capsule");
    assert(chip.querySelector("svg.lucide-image"), "image filenames need an image icon");
    assert.equal(chip.querySelector("span")?.getAttribute("title"), path);
    const remove = chip.querySelector<HTMLButtonElement>("button")!;
    assert.equal(remove.getAttribute("aria-label"), "Remove Screenshot.PNG");
    assert(remove.className.includes("focus-visible:"));
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
      assert(chip.querySelector("svg.lucide-paperclip"));
      assert(chip.classList.contains("max-w-full"));
      assert(chip.querySelector("span")?.classList.contains("truncate"));
      assert.equal(chip.querySelector("span")?.getAttribute("title"), paths[i]);
      assert(chip.querySelector("button")?.classList.contains("shrink-0"));
    }
  });
});

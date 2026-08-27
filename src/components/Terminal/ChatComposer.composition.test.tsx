// WHAT: per-engine IME composition fixtures driving the real ChatComposer handlers.
// WHY: composed text must enter the composer buffer exactly once per composition,
// and Enter during an active composition must commit, never send — engines order
// compositionstart/update/end differently (WebKit delivers the committing Enter
// AFTER compositionend with isComposing=false, keyCode 229).
// ORACLE: the textarea DOM value across every event step and forced re-renders,
// plus the chat_send_text payload captured at the bridge seam — never UI prose.
// Event-sequence driven only: no timer-scheduled coordination anywhere here.
// The harness import must stay first: react-dom freezes DOM detection at import.
import "./ChatComposer.composition.harness";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];

function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/** Drain the microtask queue deterministically (event-driven — no delays). */
async function flush(): Promise<void> {
  await act(async () => {
    await new Promise<void>((resolve) => setImmediate(resolve));
  });
}

function emptyMuxState(): ExtendedState {
  return {
    workspaces: [],
    tabs: {},
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
}

type SendCall = { tabId: string; text: string; clientToken: string };
type BridgeListener = (event: { payload: unknown }) => void;

interface Harness {
  el: HTMLTextAreaElement;
  root: Root;
  host: HTMLDivElement;
  sendCalls: SendCall[];
  listeners: Map<string, BridgeListener>;
  renders: () => number;
}

async function renderComposer(tabId: string): Promise<Harness> {
  const sendCalls: SendCall[] = [];
  const listeners = new Map<string, BridgeListener>();
  window.__composerPerf = {
    inputMarks: [],
    paintLatencies: [],
    renderCounts: {},
    compositionEvents: [],
    ignoredKeys: [],
  };
  __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
    if (command === "chat_send_text") {
      const call = args as unknown as SendCall;
      sendCalls.push(call);
      return {
        send_id: "send-1",
        tab_id: call.tabId,
        state: "accepted",
        error: null,
        ledger_id: "ledger-1",
        sequence: 1,
        retained_from: 0,
      };
    }
    if (command === "list_agent_providers" || command === "delayed_send_list") return [];
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  __setListenForTest((async (event: string, handler: BridgeListener) => {
    listeners.set(event, handler);
    return () => listeners.delete(event);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const store = createMuxStateStore(emptyMuxState);
  await act(async () => {
    root.render(
      <MuxStateStoreContext.Provider value={store}>
        <ChatComposer
          tabId={tabId}
          tabProvider={null}
          currentPtyId={null}
          pendingPrompt={null}
          attachments={[]}
          addAttachments={() => {}}
          removeAttachment={() => {}}
          clearAttachments={() => {}}
          handlePasteEvent={() => {}}
          onWillSendMessage={() => {}}
          addOptimistic={() => "optimistic-1"}
          updateOptimistic={() => {}}
          observeChatSendStatus={() => {}}
          showComposerError={() => {}}
        />
      </MuxStateStoreContext.Provider>,
    );
  });
  await flush();
  const el = host.querySelector('[data-testid="chat-composer"]');
  assert(el instanceof window.HTMLTextAreaElement, "composer textarea rendered");
  const renders = () => window.__composerPerf?.renderCounts[tabId] ?? 0;
  return { el, root, host, sendCalls, listeners, renders };
}

async function teardown(h: Harness): Promise<void> {
  await act(async () => h.root.unmount());
  h.host.remove();
  __setInvokeForTest(null);
  __setListenForTest(null);
}

function setDomValue(el: HTMLTextAreaElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")?.set;
  assert(setter, "textarea value setter available");
  setter.call(el, value);
}

async function fire(el: HTMLTextAreaElement, event: Event): Promise<void> {
  await act(async () => {
    el.dispatchEvent(event);
  });
}

function compositionEvent(type: "compositionstart" | "compositionupdate" | "compositionend", data: string): Event {
  const event = new window.Event(type, { bubbles: true });
  Object.defineProperty(event, "data", { value: data });
  return event;
}

function inputEvent(opts: { isComposing: boolean; data?: string | null; inputType?: string }): Event {
  const event = new window.Event("input", { bubbles: true });
  Object.defineProperty(event, "isComposing", { value: opts.isComposing });
  Object.defineProperty(event, "inputType", {
    value: opts.inputType ?? (opts.isComposing ? "insertCompositionText" : "insertText"),
  });
  Object.defineProperty(event, "data", { value: opts.data ?? null });
  return event;
}

function keydownEvent(opts: { key: string; keyCode: number; isComposing?: boolean; shiftKey?: boolean }): Event {
  const event = new window.KeyboardEvent("keydown", {
    key: opts.key,
    shiftKey: opts.shiftKey ?? false,
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, "isComposing", { value: opts.isComposing ?? false });
  Object.defineProperty(event, "keyCode", { value: opts.keyCode });
  return event;
}

/** Force a re-render unrelated to typing (delayed-send change → setDelayedMessages). */
async function forceUnrelatedRerender(h: Harness, tabId: string): Promise<void> {
  const handler = h.listeners.get("delayed-send-changed");
  assert(handler, "delayed-send-changed listener registered");
  await act(async () => {
    handler({ payload: { tabId } });
  });
  await flush();
}

interface EngineProfile {
  name: string;
  /** Drive the composition typing phase with real per-engine event ordering.
   *  Asserts the DOM preview survives every step (React must never write a
   *  stale draft over an active composition — the WebKitGTK re-insert trigger). */
  compose(h: Harness, base: string, prefixes: string[]): Promise<void>;
  /** Deliver the engine's committing-Enter sequence, closing the composition. */
  commitEnter(h: Harness, base: string, final: string): Promise<void>;
}

async function assertPreview(h: Harness, expected: string, step: string): Promise<void> {
  assert(
    h.el.value === expected,
    `${step}: DOM preview must hold ${JSON.stringify(expected)}, got ${JSON.stringify(h.el.value)}`,
  );
}

const webview2: EngineProfile = {
  name: "WebView2 (Chromium)",
  // Chromium: first keydown fires BEFORE compositionstart with isComposing=false,
  // keyCode 229; the committing Enter fires before compositionend, isComposing=true.
  async compose(h, base, prefixes) {
    await fire(h.el, keydownEvent({ key: "Process", keyCode: 229, isComposing: false }));
    await fire(h.el, compositionEvent("compositionstart", ""));
    for (let i = 0; i < prefixes.length; i++) {
      if (i > 0) await fire(h.el, keydownEvent({ key: "Process", keyCode: 229, isComposing: true }));
      await fire(h.el, compositionEvent("compositionupdate", prefixes[i]));
      setDomValue(h.el, base + prefixes[i]);
      await fire(h.el, inputEvent({ isComposing: true, data: prefixes[i] }));
      await assertPreview(h, base + prefixes[i], `${this.name} update ${i + 1}`);
    }
  },
  async commitEnter(h, base, final) {
    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 229, isComposing: true }));
    setDomValue(h.el, base + final);
    await fire(h.el, inputEvent({ isComposing: true, data: final }));
    await fire(h.el, compositionEvent("compositionend", final));
  },
};

const wkwebview: EngineProfile = {
  name: "WKWebView (WebKit)",
  // WebKit: the committing Enter keydown arrives AFTER compositionend with
  // isComposing=false but keyCode 229 — the classic accidental-submit shape.
  async compose(h, base, prefixes) {
    for (let i = 0; i < prefixes.length; i++) {
      await fire(h.el, keydownEvent({ key: prefixes[i].slice(-1), keyCode: 229, isComposing: i > 0 }));
      if (i === 0) await fire(h.el, compositionEvent("compositionstart", ""));
      await fire(h.el, compositionEvent("compositionupdate", prefixes[i]));
      setDomValue(h.el, base + prefixes[i]);
      await fire(h.el, inputEvent({ isComposing: true, data: prefixes[i] }));
      await assertPreview(h, base + prefixes[i], `${this.name} update ${i + 1}`);
    }
  },
  async commitEnter(h, base, final) {
    setDomValue(h.el, base + final);
    await fire(h.el, inputEvent({ isComposing: true, data: final }));
    await fire(h.el, compositionEvent("compositionend", final));
    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 229, isComposing: false }));
  },
};

const webkitgtk: EngineProfile = {
  name: "WebKitGTK",
  // WebKitGTK: WebKit ordering plus duplicate input delivery per update and a
  // stray insertFromComposition input AFTER compositionend re-announcing the text.
  async compose(h, base, prefixes) {
    for (let i = 0; i < prefixes.length; i++) {
      await fire(h.el, keydownEvent({ key: prefixes[i].slice(-1), keyCode: 229, isComposing: i > 0 }));
      if (i === 0) await fire(h.el, compositionEvent("compositionstart", ""));
      await fire(h.el, compositionEvent("compositionupdate", prefixes[i]));
      setDomValue(h.el, base + prefixes[i]);
      await fire(h.el, inputEvent({ isComposing: true, data: prefixes[i] }));
      await fire(h.el, inputEvent({ isComposing: true, data: prefixes[i] }));
      await assertPreview(h, base + prefixes[i], `${this.name} update ${i + 1}`);
    }
  },
  async commitEnter(h, base, final) {
    setDomValue(h.el, base + final);
    await fire(h.el, compositionEvent("compositionend", final));
    await fire(h.el, inputEvent({ isComposing: false, inputType: "insertFromComposition", data: final }));
    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 229, isComposing: false }));
  },
};

const PROFILES = [webview2, wkwebview, webkitgtk];
const PREFIXES = ["р", "ра", "рас"];
const FINAL = "рас";

for (const profile of PROFILES) {
  test(`${profile.name}: exactly-once emission, Enter commits and never sends`, async () => {
    const tabId = `tab-${profile.name.replace(/[^a-z0-9]/gi, "-")}`;
    const h = await renderComposer(tabId);
    try {
      await profile.compose(h, "", PREFIXES);
      await assertPreview(h, FINAL, "after typing phase");
      assert(h.sendCalls.length === 0, "nothing sent during the typing phase");

      const rendersBeforeRerender = h.renders();
      await forceUnrelatedRerender(h, tabId);
      assert(h.renders() > rendersBeforeRerender, "the unrelated re-render actually happened");
      await assertPreview(h, FINAL, "after unrelated re-render mid-composition");

      await profile.commitEnter(h, "", FINAL);
      assert(h.sendCalls.length === 0, "Enter during/committing the composition commits — it never sends");
      await assertPreview(h, FINAL, "after the commit sequence");

      await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13, isComposing: false }));
      await flush();
      assert((h.sendCalls.length as number) === 1, "plain Enter after the composition closed sends exactly once");
      assert(
        h.sendCalls[0].text === FINAL,
        `payload carries the composed text exactly once, byte-identical — no cumulative prefixes (got ${JSON.stringify(h.sendCalls[0].text)})`,
      );
    } finally {
      await teardown(h);
    }
  });
}

test("Escape-cancel: a canceled composition emits nothing", async () => {
  const h = await renderComposer("tab-escape-cancel");
  try {
    await wkwebview.compose(h, "", ["ы"]);
    await assertPreview(h, "ы", "preview before cancel");

    await fire(h.el, keydownEvent({ key: "Escape", keyCode: 229, isComposing: true }));
    setDomValue(h.el, "");
    await fire(h.el, inputEvent({ isComposing: true, inputType: "deleteCompositionText", data: null }));
    await fire(h.el, compositionEvent("compositionend", ""));
    await assertPreview(h, "", "engine removed the preview on cancel");
    assert(h.sendCalls.length === 0, "cancel sent nothing");

    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13, isComposing: false }));
    await flush();
    assert(h.sendCalls.length === 0, "Enter on the empty buffer sends nothing after cancel");

    setDomValue(h.el, "ok");
    await fire(h.el, inputEvent({ isComposing: false }));
    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13, isComposing: false }));
    await flush();
    assert((h.sendCalls.length as number) === 1, "post-cancel ASCII draft sends");
    assert(
      h.sendCalls[0].text === "ok",
      `canceled composition contributed zero bytes to the buffer (got ${JSON.stringify(h.sendCalls[0].text)})`,
    );
  } finally {
    await teardown(h);
  }
});

test("ASCII negative control: plain typing is byte-identical through the same handlers", async () => {
  const h = await renderComposer("tab-ascii-control");
  try {
    const text = "plain ascii stays byte-identical 123 ~!";
    for (let i = 1; i <= text.length; i++) {
      const prefix = text.slice(0, i);
      setDomValue(h.el, prefix);
      await fire(h.el, inputEvent({ isComposing: false }));
      assert(h.el.value === prefix, `buffer mirrors keystroke ${i} byte-identically`);
    }

    await forceUnrelatedRerender(h, "tab-ascii-control");
    assert(h.el.value === text, "ASCII draft survives an unrelated re-render (state committed per keystroke)");

    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13, isComposing: false }));
    await flush();
    assert(h.sendCalls.length === 1, "plain Enter sends exactly once");
    assert(
      h.sendCalls[0].text === text,
      `ASCII payload is byte-identical (got ${JSON.stringify(h.sendCalls[0].text)})`,
    );
  } finally {
    await teardown(h);
  }
});

test("Shift+Enter after a composition inserts a newline, never sends", async () => {
  const h = await renderComposer("tab-shift-enter");
  try {
    await webview2.compose(h, "", PREFIXES);
    await webview2.commitEnter(h, "", FINAL);
    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13, isComposing: false, shiftKey: true }));
    assert(h.sendCalls.length === 0, "Shift+Enter never submits");
  } finally {
    await teardown(h);
  }
});

async function main() {
  let failures = 0;
  try {
    for (const [name, fn] of tests) {
      try {
        await fn();
        console.log(`✓ ${name}`);
      } catch (error) {
        failures++;
        console.error(`✗ ${name}`);
        console.error(error);
      }
    }
  } finally {
    await (window as unknown as { happyDOM: { abort(): Promise<void> } }).happyDOM.abort();
  }
  if (failures > 0) {
    console.error(`ChatComposer composition tests: ${failures} failed`);
    process.exit(1);
  }
  console.log("ChatComposer composition tests: all passed");
  process.exit(0);
}

void main();

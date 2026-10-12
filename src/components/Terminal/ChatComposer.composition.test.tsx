// IME Enter must commit composition without submitting; event ordering and keyCode 229 differ across engines.
import "./ChatComposer.composition.harness";
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ChatComposer } from "./ChatComposer";
import type { ChatSendStatus } from "./chatSendStatus";
import { test } from "vitest";
import { formatAttachmentMarker } from "@codeterm/chat-core";

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
  attachments: () => string[];
  replaceAttachments: (paths: string[]) => void;
  optimisticCount: () => number;
  statuses: ChatSendStatus[];
  errors: string[];
}

interface HarnessOptions {
  attachments?: string[];
  send?: (call: SendCall) => Promise<ChatSendStatus>;
}

async function renderComposer(tabId: string, options: HarnessOptions = {}): Promise<Harness> {
  const sendCalls: SendCall[] = [];
  const listeners = new Map<string, BridgeListener>();
  const statuses: ChatSendStatus[] = [];
  let optimisticCount = 0;
  const errors: string[] = [];
  const attachmentState: { current: string[]; replace: (paths: string[]) => void } = {
    current: options.attachments ?? [],
    replace: (_paths: string[]) => { throw new Error("composer not mounted"); },
  };
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
      if (options.send) return options.send(call);
      return {
        send_id: "send-1",
        tab_id: call.tabId,
        state: "held",
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
  function ComposerHarness() {
    const [attachments, setAttachments] = useState(options.attachments ?? []);
    attachmentState.current = attachments;
    attachmentState.replace = setAttachments;
    return (
      <ChatComposer
        tabId={tabId}
        tabProvider={null}
        currentPtyId={null}
        pendingPrompt={null}
        attachments={attachments}
        addAttachments={(paths) => setAttachments((current) => [...current, ...paths])}
        removeAttachment={(path) => setAttachments((current) => current.filter((item) => item !== path))}
        clearAttachments={() => setAttachments([])}
        handlePasteEvent={() => {}}
        onWillSendMessage={() => {}}
        addOptimistic={() => { optimisticCount++; return "optimistic-1"; }}
        updateOptimistic={() => {}}
        observeChatSendStatus={(status) => statuses.push(status)}
        showComposerError={(message) => errors.push(message)}
      />
    );
  }
  await act(async () => {
    root.render(
      <MuxStateStoreContext.Provider value={store}>
        <ComposerHarness />
      </MuxStateStoreContext.Provider>,
    );
  });
  await flush();
  const el = host.querySelector('[data-testid="chat-composer"]');
  assert(el instanceof window.HTMLTextAreaElement, "composer textarea rendered");
  const renders = () => window.__composerPerf?.renderCounts[tabId] ?? 0;
  return {
    el, root, host, sendCalls, listeners, renders, statuses, errors, optimisticCount: () => optimisticCount,
    attachments: () => attachmentState.current,
    replaceAttachments: (paths) => attachmentState.replace(paths),
  };
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

test("iPad software Return clears the draft and creates one optimistic message", async () => {
  const originalPlatform = Object.getOwnPropertyDescriptor(navigator, "platform");
  const originalTouchPoints = Object.getOwnPropertyDescriptor(navigator, "maxTouchPoints");
  Object.defineProperty(navigator, "platform", { configurable: true, value: "MacIntel" });
  Object.defineProperty(navigator, "maxTouchPoints", { configurable: true, value: 5 });
  const h = await renderComposer("tab-ipad-return");
  try {
    setDomValue(h.el, "explain this");
    await fire(h.el, inputEvent({ isComposing: false }));
    const shifted = keydownEvent({ key: "Enter", keyCode: 13, shiftKey: true });
    await fire(h.el, shifted);
    assert(!shifted.defaultPrevented && h.sendCalls.length === 0, "Shift+Return permits a newline");
    const enter = keydownEvent({ key: "Enter", keyCode: 13 });
    await fire(h.el, enter);
    await flush();
    assert(enter.defaultPrevented, "Return prevents the browser newline");
    assert((h.sendCalls.length as number) === 1 && h.sendCalls[0].text === "explain this", "Return submits the exact draft once");
    assert(h.el.value === "" && h.optimisticCount() === 1, "the composer clears and shows one optimistic message");
  } finally {
    await teardown(h);
    if (originalPlatform) Object.defineProperty(navigator, "platform", originalPlatform);
    else Reflect.deleteProperty(navigator, "platform");
    if (originalTouchPoints) Object.defineProperty(navigator, "maxTouchPoints", originalTouchPoints);
    else Reflect.deleteProperty(navigator, "maxTouchPoints");
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function heldStatus(tabId: string): ChatSendStatus {
  return {
    send_id: "opaque-send-a", tab_id: tabId, state: "held", error: null,
    ledger_id: "opaque-ledger", sequence: 1, retained_from: 1,
  };
}

for (const [name, refusal] of [
  ["capacity refusal", new Error("chat send backpressure")],
  ["storage refusal", new Error("opaque-storage-refusal")],
  ["ambiguous transport completion", new TypeError("opaque-transport-disconnect")],
] as const) {
  test(`${name} clears the draft and attachments without resubmitting`, async () => {
    const admission = deferred<ChatSendStatus>();
    const text = "opaque-draft-5ab1";
    const attachment = "/opaque/attachment-8c2f.txt";
    const h = await renderComposer(`tab-admission-${name}`, {
      attachments: [attachment], send: () => admission.promise,
    });
    try {
      setDomValue(h.el, text);
      await fire(h.el, inputEvent({ isComposing: false }));
      await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13 }));
      await flush();
      assert(h.sendCalls.length === 1, "the actual composer invoked admission once");
      assert(h.sendCalls[0].text === `${text} ${formatAttachmentMarker(attachment)}`, "the complete draft and attachment reached admission");
      assert(h.statuses.length === 0, "a pending request is not an accepted receipt");
      await act(async () => admission.reject(refusal));
      await flush();

      assert(h.el.value === "", "rejection never restores the sent draft");
      assert(h.attachments().length === 0, "sent attachments leave the composer");
      assert(h.statuses.length === 0, "a rejection must not fabricate acceptance or delivery");
      assert(h.errors.length === 0, "delivery state adds no composer error UI");
      await forceUnrelatedRerender(h, `tab-admission-${name}`);
      assert(h.sendCalls.length === 1, "an ambiguous completion never triggers an automatic resend");
    } finally {
      admission.reject(refusal);
      await teardown(h);
    }
  });
}

for (const outcome of ["accepted", "refused", "ambiguous"] as const) {
  test(`asynchronous ${outcome} admission preserves a newer edited draft and attachment`, async () => {
    const tabId = `tab-newer-admission-draft-${outcome}`;
    const admission = deferred<ChatSendStatus>();
    const h = await renderComposer(tabId, { send: () => admission.promise });
    try {
      setDomValue(h.el, "opaque-first-6bc3");
      await fire(h.el, inputEvent({ isComposing: false }));
      await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13 }));
      await flush();
      assert(h.sendCalls.length === 1, "first request reached admission");

      const newer = "opaque-newer-7dc4";
      setDomValue(h.el, newer);
      await fire(h.el, inputEvent({ isComposing: false }));
      await act(async () => h.replaceAttachments(["/opaque/newer-9ed5.txt"]));
      await act(async () => {
        if (outcome === "accepted") admission.resolve(heldStatus(tabId));
        else admission.reject(new Error(`opaque-${outcome}-completion`));
      });
      await flush();

      assert(h.el.value === newer, "completion of the older request must not clear a newer edit");
      assert(JSON.stringify(h.attachments()) === JSON.stringify(["/opaque/newer-9ed5.txt"]), "the newer attachment survives");
      if (outcome === "accepted") {
        assert(h.statuses.length === 1 && h.statuses[0].state === "held", "the durable hold receipt is observed once");
      } else {
        assert(h.statuses.length === 0, "an unacknowledged admission is not acceptance");
      }
      assert(h.sendCalls.length === 1, "the newer draft remains unsent");
    } finally {
      admission.resolve(heldStatus(tabId));
      await teardown(h);
    }
  });
}

test("send clears immediately and an admission receipt keeps the composer empty", async () => {
  const tabId = "tab-unchanged-admission";
  const admission = deferred<ChatSendStatus>();
  const h = await renderComposer(tabId, {
    attachments: ["/opaque/attachment-unchanged.txt"], send: () => admission.promise,
  });
  const composerValue = () => h.el.value;
  const statusCount = () => h.statuses.length;
  try {
    const text = "opaque-unchanged-draft";
    setDomValue(h.el, text);
    await fire(h.el, inputEvent({ isComposing: false }));
    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13 }));
    await flush();
    assert(h.sendCalls.length === 1, "one request reached the real composer transport");
    assert(composerValue() === "", "send clears before admission resolves");
    assert(h.attachments().length === 0, "send clears attachments before admission resolves");
    assert(statusCount() === 0, "pending transport supplies no receipt");
    await act(async () => admission.resolve(heldStatus(tabId)));
    await flush();
    assert(composerValue() === "", "the admitted unchanged draft clears");
    assert(h.attachments().length === 0, "the admitted unchanged attachment clears");
    assert(statusCount() === 1, "exactly one actual acceptance is observed");
  } finally {
    admission.resolve(heldStatus(tabId));
    await teardown(h);
  }
});

test("admission preserves a draft edited away and back to the same text", async () => {
  const tabId = "tab-revised-identical-draft";
  const admission = deferred<ChatSendStatus>();
  const h = await renderComposer(tabId, { send: () => admission.promise });
  try {
    const text = "opaque-revised-original";
    setDomValue(h.el, text);
    await fire(h.el, inputEvent({ isComposing: false }));
    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13 }));
    await flush();
    for (const edit of ["opaque-intermediate-edit", text]) {
      setDomValue(h.el, edit);
      await fire(h.el, inputEvent({ isComposing: false }));
    }
    await act(async () => admission.resolve(heldStatus(tabId)));
    await flush();
    assert(h.el.value === text, "a newer draft with equal text retains its own ownership");
    assert(h.sendCalls.length === 1, "the new equal-text draft is not automatically submitted");
    assert(h.statuses.length === 1, "the older acceptance is still observed");
  } finally {
    admission.resolve(heldStatus(tabId));
    await teardown(h);
  }
});


test("typed model commands use the ordinary send path without a composer picker", async () => {
  const h = await renderComposer("tab-model-command");
  try {
    setDomValue(h.el, "/model fixture-model");
    await fire(h.el, inputEvent({ isComposing: false }));
    assert(!h.host.querySelector('[aria-label="Model and traits"]'), "model chip stays hidden");
    assert(!h.host.querySelector('[aria-label="Select model"]'), "model dialog stays hidden");
    await fire(h.el, keydownEvent({ key: "Enter", keyCode: 13 }));
    await flush();
    assert(h.sendCalls.length === 1, "command is submitted once");
    assert(h.sendCalls[0].text === "/model fixture-model", "command reaches the existing send path unchanged");
  } finally {
    await teardown(h);
  }
});

test("repeated pending Enter presses create one bubble and one request", async () => {
  const admission = deferred<ChatSendStatus>();
  const h = await renderComposer("tab-repeat-pending", { send: () => admission.promise });
  try {
    setDomValue(h.el, "opaque-repeat");
    await fire(h.el, inputEvent({ isComposing: false }));
    await act(async () => {
      h.el.dispatchEvent(keydownEvent({ key: "Enter", keyCode: 13 }));
      h.el.dispatchEvent(keydownEvent({ key: "Enter", keyCode: 13 }));
    });
    await flush();
    assert(h.el.value === "", "pending send clears immediately");
    assert(h.sendCalls.length === 1, "repeated press cannot admit another transaction");
    assert(h.optimisticCount() === 1, "repeated press cannot create another bubble");
  } finally {
    admission.resolve(heldStatus("tab-repeat-pending"));
    await teardown(h);
  }
});

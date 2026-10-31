import { harnessWindow as browser } from "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { ComposerMentionPicker } from "./ComposerMentionPicker";
import { useComposerMentions } from "./useComposerMentions";
import type { ComposerMention, MentionScope, createMentionRegistry } from "../../lib/composerMentions";

const initialScope: MentionScope = { tabId: "self", workspaceId: "ws", projectPath: "/repo", cwd: "/repo", teamId: "own" };
const row = (id: string): ComposerMention => ({ id, label: id, description: "Worker · A long description", insertText: `@${id} (tab ${id})`, group: "Tabs", kind: "tab", match: 1 });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const requests: { query: string; scope: MentionScope; signal: AbortSignal; response: ReturnType<typeof deferred<ComposerMention[]>> }[] = [];
let subscriptions = 0;
let closed = 0;
let invalidate!: () => void;
const registry: ReturnType<typeof createMentionRegistry> = {
  search(query, context) {
    const response = deferred<ComposerMention[]>();
    requests.push({ query, ...context, response });
    return response.promise;
  },
  subscribe(changed) { invalidate = changed; subscriptions++; return () => { subscriptions--; }; },
  close() { closed++; },
};
let change!: (draft: string) => void;
let changeScope!: (scope: MentionScope) => void;
let stalePick!: () => void;
let currentScope = initialScope;
function Harness() {
  const [draft, setDraft] = useState("@");
  const [scope, setScope] = useState(initialScope);
  currentScope = scope;
  const ref = useRef<HTMLTextAreaElement>(null);
  const picker = useComposerMentions(draft, setDraft, () => ref.current?.focus(), ref, registry, () => currentScope);
  changeScope = setScope;
  change = (text) => {
    const editor = ref.current!;
    Object.getOwnPropertyDescriptor(browser.HTMLTextAreaElement.prototype, "value")!.set!.call(editor, text);
    editor.setSelectionRange(text.length, text.length);
    setDraft(text);
    picker.syncSelection(editor);
  };
  if (picker.mentions.length) stalePick = () => picker.pick(picker.mentions[0]);
  return <><textarea ref={ref} value={draft} onChange={(event) => { setDraft(event.currentTarget.value); picker.syncSelection(event.currentTarget); }} onKeyDown={picker.handleKey} />
    <ComposerMentionPicker key={picker.session} {...picker} onHighlight={picker.setHighlighted} onPick={picker.pick} onClose={picker.close} triggerRef={ref} /></>;
}
async function main() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const list = () => document.querySelector<HTMLElement>('[role="listbox"]')!;
  const options = () => Array.from(document.querySelectorAll('[role="option"]')).map((element) => element.getAttribute("aria-label"));
  const newest = () => requests[requests.length - 1];
  const key = async (code: string) => act(async () => { host.querySelector("textarea")!.dispatchEvent(new browser.KeyboardEvent("keydown", { key: code, code, bubbles: true, cancelable: true }) as unknown as Event); });
  try {
    await act(async () => root.render(<Harness />));
    const editor = host.querySelector("textarea")!;
    editor.focus();
    assert.equal(editor.getAttribute("role"), "combobox");
    assert.equal(editor.getAttribute("aria-controls"), list().id);
    assert.equal(list().getAttribute("aria-busy"), "true");
    await act(async () => newest().response.resolve([row("Rivet"), row("River")]));
    await key("ArrowDown");
    const panel = list();
    const activeId = editor.getAttribute("aria-activedescendant");
    panel.scrollTop = 27;
    for (const text of ["@r", "@ri", "@riv", "@rive", "@rivet"]) {
      await act(async () => change(text));
      assert.equal(list(), panel, `${text} retains the listbox DOM`);
      assert.deepEqual(options(), ["Rivet", "River"]);
      assert.equal(editor.getAttribute("aria-activedescendant"), activeId);
      assert.equal(document.getElementById(activeId!)?.getAttribute("aria-selected"), "true");
      assert.equal(list().getAttribute("aria-busy"), "true");
      assert.equal(editor.getAttribute("aria-busy"), "true");
      assert.equal(panel.scrollTop, 27);
      assert.equal(document.activeElement, editor);
    }
    for (const request of requests.slice(2, -1).reverse()) {
      assert.equal(request.signal.aborted, true);
      await act(async () => request.response.resolve([row(`late-${request.query}`)]));
      assert.deepEqual(options(), ["Rivet", "River"], "obsolete completion cannot replace retained rows");
    }
    await act(async () => newest().response.resolve([row("River"), row("Rivet")]));
    assert.equal(list(), panel);
    assert.equal(editor.getAttribute("aria-activedescendant"), activeId, "selection follows identity through reorder");
    assert.equal(list().getAttribute("aria-busy"), "false");
    await act(async () => requests[1].response.resolve([row("late-after-newest")]));
    assert.deepEqual(options(), ["River", "Rivet"], "old completion after newest settlement cannot win");
    assert.equal(editor.getAttribute("aria-activedescendant"), activeId);
    await act(async () => invalidate());
    assert.equal(list(), panel, "subscription refresh retains the same panel");
    assert.equal(list().getAttribute("aria-busy"), "true");
    await act(async () => newest().response.resolve([row("River"), row("Rivet")]));
    assert.equal(editor.getAttribute("aria-activedescendant"), activeId);
    await key("Tab");
    assert.equal(editor.value, "@River (tab River) ");
    assert.equal(editor.selectionStart, editor.textLength);
    assert.equal(document.activeElement, editor);
    assert.equal(subscriptions, 0);
    assert.equal(editor.getAttribute("aria-expanded"), "false");
    assert.equal(editor.hasAttribute("aria-activedescendant"), false);

    await act(async () => change("@"));
    await act(async () => newest().response.resolve([row("Own")]));
    const oldPick = stalePick;
    await act(async () => change("@ @"));
    assert.deepEqual(options(), [], "new token clears prior token rows immediately");
    await act(async () => oldPick());
    assert.equal(editor.value, "@ @", "stale token handler cannot insert");
    await act(async () => newest().response.resolve([row("Own")]));
    const scopePick = stalePick;
    currentScope = { ...initialScope, teamId: "changed-before-render" };
    await act(async () => scopePick());
    assert.equal(editor.value, "@ @", "live scope is rechecked at insertion before any rerender");
    currentScope = initialScope;
    await act(async () => changeScope({ ...initialScope, teamId: "other" }));
    assert.deepEqual(options(), [], "other Team never sees retained own-Team rows");
    await act(async () => scopePick());
    assert.equal(editor.value, "@ @");
    const otherRequest = newest();
    await act(async () => changeScope({ ...initialScope, tabId: "another" }));
    await act(async () => otherRequest.response.resolve([row("Foreign")]));
    assert.deepEqual(options(), []);
    for (const changed of [
      { ...initialScope, workspaceId: "another-workspace" },
      { ...initialScope, projectPath: "/other-project" },
      { ...initialScope, cwd: "/other-directory" },
    ]) {
      const obsolete = newest();
      await act(async () => changeScope(changed));
      await act(async () => obsolete.response.resolve([row("obsolete-scope")]));
      assert.deepEqual(options(), [], "every scope field invalidates retained rows and pending completion");
    }
    await act(async () => newest().response.resolve([]));
    const emptyPanel = list();
    assert.equal(emptyPanel.getAttribute("aria-busy"), "false");
    assert.ok(emptyPanel.textContent?.includes("No matches"));
    assert.equal(editor.hasAttribute("aria-activedescendant"), false);
    await act(async () => change("@ @err"));
    assert.equal(list(), emptyPanel);
    await act(async () => newest().response.reject(new Error("fixture failure")));
    assert.equal(list(), emptyPanel);
    assert.ok(list().textContent?.includes("Suggestions unavailable"));
    assert.equal(list().getAttribute("aria-busy"), "false");
    await key("Escape");
    assert.equal(list(), null);
    const dismissedDraft = editor.value;
    await act(async () => change(`${dismissedDraft}x`));
    assert.equal(list(), null, "dismissal persists through typing");
    await act(async () => change("@ @e"));
    assert.ok(list(), "deleting into token reopens");
    await act(async () => newest().response.resolve([row("One"), row("Two"), row("Three")]));
    await key("End");
    assert.equal(document.querySelector('[aria-selected="true"]')?.getAttribute("aria-label"), "Three");
    await key("Home"); await key("ArrowUp");
    assert.equal(document.querySelector('[aria-selected="true"]')?.getAttribute("aria-label"), "Three");
    await key("PageUp"); await key("PageDown"); await key("Enter");
    assert.equal(editor.value, "@ @Three (tab Three) ");
    await act(async () => change("@"));
    await act(async () => newest().response.resolve([row("Touch")]));
    const option = document.querySelector<HTMLElement>('[role="option"]')!;
    const pointer = new browser.PointerEvent("pointerdown", { bubbles: true, cancelable: true, pointerType: "touch" });
    await act(async () => { option.dispatchEvent(pointer as unknown as Event); option.click(); });
    assert.equal(pointer.defaultPrevented, true);
    assert.equal(editor.value, "@Touch (tab Touch) ");
    assert.equal(document.activeElement, editor);
    await act(async () => change("@"));
    await act(async () => newest().response.resolve([row("Mouse")]));
    const mouseOption = document.querySelector<HTMLElement>('[role="option"]')!;
    const mouse = new browser.MouseEvent("mousedown", { bubbles: true, cancelable: true });
    await act(async () => { mouseOption.dispatchEvent(mouse as unknown as Event); mouseOption.click(); });
    assert.equal(mouse.defaultPrevented, true);
    assert.equal(editor.value, "@Mouse (tab Mouse) ");
    assert.equal(editor.selectionStart, editor.textLength);
    assert.equal(document.activeElement, editor);
    await act(async () => change("@"));
    await act(async () => document.querySelector<HTMLElement>('[aria-label="Close mentions"]')!.click());
    assert.equal(editor.value, "@"); assert.equal(list(), null);
    await act(async () => change("@ @"));
    const pending = newest();
    await act(async () => root.unmount());
    assert.equal(pending.signal.aborted, true);
    await act(async () => pending.response.resolve([row("after-unmount")]));
    assert.equal(subscriptions, 0); assert.ok(closed > 0);
    assert.equal(document.querySelector('[role="listbox"]'), null);
    console.log("PASS scoped refresh, five pending paints, out-of-order rejection, selection identity, empty/error, scope/token rejection, keyboard/touch, dismissal and cleanup");
  } finally { await act(async () => root.unmount()); host.remove(); browser.happyDOM.abort(); }
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });

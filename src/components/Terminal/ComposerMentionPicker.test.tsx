import { harnessWindow as browser } from "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { ComposerMentionPicker } from "./ComposerMentionPicker";
import { useComposerMentions } from "./useComposerMentions";
import { createMentionRegistry, MENTION_PICKER_LAYOUT, type MentionScope } from "../../lib/composerMentions";
import { anchoredPopoverPosition } from "../../../packages/chat-ui/src/floatingPosition";
const scope: MentionScope = { tabId: "self", workspaceId: "ws", projectPath: "/repo", teamId: null };
let searchCalls = 0;
let subscriptions = 0;
let releaseSearch!: () => void;
let pendingSearch: Promise<void> | undefined = new Promise((resolve) => { releaseSearch = resolve; });
const registry = createMentionRegistry([{ id: "fixture", group: "Tabs", async search() { searchCalls++; await pendingSearch; return [
  { id: "a", label: "Rivet", description: "Worker · Builds", insertText: "@Rivet (tab a)", group: "Tabs", match: 1 },
  { id: "b", label: "Tally", description: "Worker · Mentions", insertText: "@Tally (tab b)", group: "Tabs", match: 1 },
]; } }], { subscribe: () => { subscriptions++; return () => { subscriptions--; }; } });
let change!: (text: string) => void;
let moveCaret!: (caret: number) => void;
function Harness() {
  const [draft, setDraft] = useState("@");
  const ref = useRef<HTMLTextAreaElement>(null);
  const picker = useComposerMentions(draft, setDraft, () => ref.current?.focus(), ref, registry, scope);
  moveCaret = (caret) => { ref.current!.setSelectionRange(caret, caret); picker.syncSelection(ref.current!); };
  change = (text) => {
    const editor = ref.current!;
    Object.getOwnPropertyDescriptor(browser.HTMLTextAreaElement.prototype, "value")!.set!.call(editor, text);
    editor.setSelectionRange(text.length, text.length);
    setDraft(text);
    picker.syncSelection(editor);
  };
  return <><textarea ref={ref} value={draft} onChange={(event) => { setDraft(event.currentTarget.value); picker.syncSelection(event.currentTarget); }} onSelect={(event) => picker.syncSelection(event.currentTarget)} onKeyDown={picker.handleKey} />
    <ComposerMentionPicker key={picker.session} {...picker} onHighlight={picker.setHighlighted} onPick={picker.pick} onClose={picker.close} triggerRef={ref} /></>;
}
async function main() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const key = async (code: string) => act(async () => { host.querySelector("textarea")!.dispatchEvent(new browser.KeyboardEvent("keydown", { key: code, code, bubbles: true, cancelable: true }) as unknown as Event); });
  try {
    await act(async () => root.render(<Harness />));
    assert.ok(document.querySelector('[role="status"]'), "loading suggestions are visible before the query resolves");
    assert.equal(document.querySelectorAll('[role="option"]').length, 0);
    await act(async () => { releaseSearch(); pendingSearch = undefined; });
    assert.equal(document.querySelector('[role="status"]'), null);
    assert.equal(document.querySelectorAll('[role="option"]').length, 2);
    await key("ArrowDown"); await key("Tab");
    assert.equal(host.querySelector("textarea")!.value, "@Tally (tab b) ");
    await act(async () => { change("@"); });
    const input = host.querySelector("textarea")!;
    await act(async () => { input.setSelectionRange(1, 1); input.dispatchEvent(new browser.Event("select", { bubbles: true }) as unknown as Event); });
    await key("Escape");
    assert.equal(input.value, "@");
    assert.equal(document.querySelector('[role="listbox"]'), null);
    await act(async () => { change("@t"); });
    assert.equal(document.querySelector('[role="listbox"]'), null, "typing after Escape stays closed");
    await act(async () => { change("@"); input.setSelectionRange(1, 1); input.dispatchEvent(new browser.Event("select", { bubbles: true }) as unknown as Event); });
    assert.ok(document.querySelector('[role="listbox"]'), "backspace reopens");
    await act(async () => { document.body.dispatchEvent(new browser.PointerEvent("pointerdown", { bubbles: true, pointerType: "touch" }) as unknown as Event); });
    assert.equal(document.querySelector('[role="listbox"]'), null);
    assert.equal(input.value, "@");
    await act(async () => { change("@ @"); });
    await act(async () => { input.setSelectionRange(3, 3); input.dispatchEvent(new browser.Event("select", { bubbles: true }) as unknown as Event); });
    const option = document.querySelector<HTMLDivElement>('[role="option"]'); assert.ok(option);
    await act(async () => { option.dispatchEvent(new browser.PointerEvent("pointerdown", { bubbles: true, pointerType: "touch" }) as unknown as Event); option.click(); });
    assert.equal(input.value, "@ @Rivet (tab a) ");
    const position = anchoredPopoverPosition({ left: 8, right: 382, top: 310, bottom: 350 }, { width: 374, height: 264 }, { width: 390, height: 370 }, MENTION_PICKER_LAYOUT.placement, { keepSide: true });
    assert.equal(position.side, "top"); assert.ok(position.top + position.maxHeight <= 310, "phone input and send row remain visible above keyboard");
    const cramped = anchoredPopoverPosition({ left: 8, right: 382, top: 40, bottom: 80 }, { width: 374, height: 264 }, { width: 390, height: 400 }, "top-start", { keepSide: true });
    assert.equal(cramped.side, "bottom", "short upper pane falls back to the useful side");
    assert.ok(cramped.top >= 80 && cramped.maxHeight >= 96, "fallback keeps a useful visible picker outside the input");
    await act(async () => { change("@ta"); });
    const searches = searchCalls;
    await act(async () => { moveCaret(2); });
    assert.equal(searchCalls, searches, "caret moves within a token do not search again");
    assert.equal(subscriptions, 1);
    await act(async () => { input.dispatchEvent(new browser.KeyboardEvent("keydown", { key: "Enter", code: "", bubbles: true, cancelable: true }) as unknown as Event); });
    assert.equal(input.value, "@Rivet (tab a) ");
    assert.equal(subscriptions, 0, "completed/closed picker releases subscriptions");
    console.log("8 picker behavior/layout/subscription checks passed");
  } finally { await act(async () => root.unmount()); browser.happyDOM.abort(); }
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });

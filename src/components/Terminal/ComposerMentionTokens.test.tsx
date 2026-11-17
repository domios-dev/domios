import { harnessWindow as browser } from "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { DOMIOS_TEXT_MIME } from "@codeterm/chat-core";
import { useComposerMentionTokens } from "./useComposerMentionTokens";
import { ComposerMentionHighlight } from "./ComposerMentionHighlight";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { trackComposerMentionDrop } from "../Sidebar/composerMentionDrop";

const marker = '<domios type="tab" id="elgar" />';
const state = { tabs: { elgar: { id: "elgar", title: "Elgar" } }, workspaces: [] } as unknown as ExtendedState;
const mux = createMuxStateStore(() => state);
let canonical = "";
let insert!: () => void;
let commands = 0;
Object.assign(document, { execCommand: (command: string, _ui: boolean, text = "") => {
  commands++;
  const editor = document.activeElement as HTMLTextAreaElement;
  const start = editor.selectionStart;
  const next = editor.value.slice(0, start) + text + editor.value.slice(editor.selectionEnd);
  Object.getOwnPropertyDescriptor(browser.HTMLTextAreaElement.prototype, "value")!.set!.call(editor, next);
  editor.setSelectionRange(start + text.length, start + text.length);
  editor.dispatchEvent(new browser.InputEvent("input", { inputType: command === "delete" ? "deleteContentBackward" : "insertText", bubbles: true }) as unknown as Event);
  return true;
} });

function Harness() {
  const [draft, setDraft] = useState("Hello ");
  canonical = draft;
  const editor = useRef<HTMLTextAreaElement>(null);
  const mentions = useComposerMentionTokens(draft, setDraft, editor);
  insert = () => mentions.insert({ id: "tabs:elgar", label: "Elgar", insertText: marker, kind: "tab", group: "Tabs", description: "", match: 0 },
    { start: editor.current!.selectionStart, end: editor.current!.selectionEnd, partial: "" });
  return <div style={{ position: "relative" }}>
    <ComposerMentionHighlight draft={mentions.draft} textarea={editor} marginPx={0} />
    <textarea data-testid="chat-composer" ref={editor} value={mentions.draft.text}
      onChange={event => mentions.change(event.currentTarget.value, (event.nativeEvent as InputEvent).inputType)}
      onBeforeInput={mentions.beforeInput} onKeyDown={mentions.handleKey} onSelect={event => mentions.snapSelection(event.currentTarget)}
      onCopy={event => mentions.handleCopy(event, false)} onCut={event => mentions.handleCopy(event, true)} onPaste={mentions.handlePaste} />
    <span role="status" aria-live="polite">{mentions.announcement}</span>
  </div>;
}

async function main() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MuxStateStoreContext value={mux}><Harness /></MuxStateStoreContext>));
    const editor = host.querySelector("textarea")!;
    await act(async () => { editor.focus(); editor.setSelectionRange(6, 6); insert(); });
    assert.equal(editor.value, "Hello @Elgar ");
    assert.equal(canonical, `Hello ${marker} `);
    assert.equal(commands, 1, "picker insertion uses one native editing command");
    assert.equal(host.querySelector('[role="status"]')?.textContent, "Mentioned Elgar");
    await act(async () => {
      editor.setSelectionRange(12, 12);
      editor.dispatchEvent(new browser.KeyboardEvent("keydown", { code: "Backspace", bubbles: true, cancelable: true }) as unknown as Event);
    });
    assert.equal(editor.value, "Hello  ");
    assert.equal(commands, 2, "whole token deletion uses one native editing command");
    assert.equal(host.querySelector('[role="status"]')?.textContent, "Removed Elgar");
    await act(async () => {
      Object.getOwnPropertyDescriptor(browser.HTMLTextAreaElement.prototype, "value")!.set!.call(editor, "Hello @Elgar ");
      editor.dispatchEvent(new browser.InputEvent("input", { inputType: "historyUndo", bubbles: true }) as unknown as Event);
    });
    assert.equal(canonical, `Hello ${marker} `, "native undo input restores the marker");
    const clipboard = new browser.DataTransfer();
    await act(async () => {
      editor.setSelectionRange(7, 10);
      editor.dispatchEvent(new browser.ClipboardEvent("copy", { clipboardData: clipboard, bubbles: true, cancelable: true }) as unknown as Event);
    });
    assert.equal(clipboard.getData("text/plain"), "@Elgar");
    assert.equal(clipboard.getData(DOMIOS_TEXT_MIME), marker);
    await act(async () => {
      editor.dispatchEvent(new browser.ClipboardEvent("cut", { clipboardData: clipboard, bubbles: true, cancelable: true }) as unknown as Event);
    });
    assert.equal(canonical, "Hello  ");
    await act(async () => {
      editor.dispatchEvent(new browser.ClipboardEvent("paste", { clipboardData: clipboard, bubbles: true, cancelable: true }) as unknown as Event);
    });
    assert.equal(editor.value, "Hello @Elgar  ");
    assert.equal(canonical, `Hello ${marker}  `);
    const raw = new browser.DataTransfer(); raw.setData("text/plain", "(tab elgar)");
    await act(async () => {
      editor.setSelectionRange(0, editor.value.length);
      editor.dispatchEvent(new browser.ClipboardEvent("paste", { clipboardData: raw, bubbles: true, cancelable: true }) as unknown as Event);
    });
    assert.equal(editor.value, "@Elgar ");
    assert.equal(canonical, `${marker} `);
    Object.assign(editor, { getBoundingClientRect: () => ({ left: 100, right: 300, top: 100, bottom: 150, width: 200, height: 50 }) });
    await act(async () => {
      editor.setSelectionRange(editor.value.length, editor.value.length);
      const finish = trackComposerMentionDrop();
      document.dispatchEvent(new browser.MouseEvent("mousemove", { clientX: 200, clientY: 120 }) as unknown as Event);
      assert.equal(finish("elgar"), true);
    });
    assert.equal(canonical, `${marker} ${marker} `, "sidebar drop inserts a canonical token");
    assert.equal(editor.value, "@Elgar @Elgar ");
  } finally { await act(async () => root.unmount()); host.remove(); }
  console.log("Composer mention native command, undo input, clipboard, raw paste and sidebar drop wiring checks passed");
}
void main().catch(error => { console.error(error); process.exitCode = 1; });

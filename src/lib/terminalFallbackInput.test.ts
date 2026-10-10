import "../components/Terminal/ChatComposer.composition.harness";
import { strict as assert } from "node:assert";
import { forwardTerminalFallback, terminalFallbackData, type FallbackKeyEvent } from "./terminalFallbackInput";
import { resolveInitialTabUiMode } from "./clientTabUiMode";

const ev = (key: string, over: Partial<FallbackKeyEvent> = {}): FallbackKeyEvent => ({
  altKey: false,
  ctrlKey: false,
  isComposing: false,
  key,
  keyCode: 0,
  metaKey: false,
  shiftKey: false,
  ...over,
});

assert.equal(terminalFallbackData(ev("a")), "a");
assert.equal(terminalFallbackData(ev("Enter")), "\r");
assert.equal(terminalFallbackData(ev("ArrowUp")), "\x1b[A");
assert.equal(terminalFallbackData(ev("c", { ctrlKey: true })), "\x03");

assert.equal(terminalFallbackData(ev("з")), "", "non-ASCII printable text must stay on xterm/IME path");
assert.equal(terminalFallbackData(ev("з", { isComposing: true })), "", "composition keydown is never forwarded");
assert.equal(terminalFallbackData(ev("Process", { keyCode: 229 })), "", "IME process key is never forwarded");
assert.equal(terminalFallbackData(ev("Dead")), "", "dead keys are never forwarded");

const writes: string[] = [];
const write = (data: string) => { writes.push(data); };
const keydown = () => new KeyboardEvent("keydown", { key: "Enter", cancelable: true });

for (const tag of ["textarea", "input", "select", "button"]) {
  const input = document.createElement(tag);
  input.setAttribute("data-testid", "chat-composer");
  document.body.append(input);
  input.focus();
  const event = keydown();
  assert.equal(document.activeElement, input, `${tag} owns focus`);
  assert.equal(forwardTerminalFallback(event, document.activeElement, write, "terminal"), false);
  assert.equal(event.defaultPrevented, false, `${tag} retains its key`);
  input.remove();
}

const editor = document.createElement("div");
editor.contentEditable = "true";
assert.equal(forwardTerminalFallback(keydown(), editor, write), false, "editable input keeps its keys");
const textbox = document.createElement("div");
textbox.setAttribute("role", "textbox");
const child = document.createElement("span");
textbox.append(child);
assert.equal(forwardTerminalFallback(keydown(), child, write), false, "textbox descendants keep their keys");

const surface = document.createElement("div");
surface.tabIndex = 0;
document.body.append(surface);
surface.focus();
const chatKey = keydown();
assert.equal(forwardTerminalFallback(chatKey, document.activeElement, write, "chat"), false);
assert.equal(chatKey.defaultPrevented, false, "Chat mode retains keys even without composer focus");
assert.equal(forwardTerminalFallback(
  keydown(), surface, write,
  resolveInitialTabUiMode("mesh:peer/initial-chat-tab", { chatCapable: true }),
), false, "initial Chat mode blocks fallback before the current mode is published");
const consumedKey = keydown();
consumedKey.preventDefault();
assert.equal(forwardTerminalFallback(consumedKey, surface, write), false, "consumed keys cannot be forwarded twice");
assert.deepEqual(writes, [], "focused inputs and Chat mode cause zero PTY writes");
const terminalKey = keydown();
assert.equal(forwardTerminalFallback(terminalKey, surface, write, "terminal"), true);
assert.equal(terminalKey.defaultPrevented, true);
assert.deepEqual(writes, ["\r"], "terminal fallback forwards one key");
surface.remove();

console.log("terminalFallbackInput.test.ts: ok");

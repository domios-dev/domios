import { strict as assert } from "node:assert";
import { terminalFallbackData, type FallbackKeyEvent } from "./terminalFallbackInput";

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

console.log("terminalFallbackInput.test.ts: ok");

// WHAT: Submit-boundary undo retirement with an injected editable-root reset port.
// WHY: Native helper history must not retain previous submissions or interrupt editing.
// ORACLE: Three submissions retain zero modeled helper entries; editing/IME and foreign history survive.
import assert from "node:assert/strict";
import { test } from "node:test";
import { createTerminalUndoBoundary } from "./terminalUndoBoundary";

function fixture() {
  const scheduled: Array<() => void> = [];
  const helper: string[] = [];
  const foreign = ["composer draft"];
  let resets = 0;
  const boundary = createTerminalUndoBoundary({
    schedule: callback => scheduled.push(callback),
    reset: () => { helper.length = 0; resets++; },
  });
  return { boundary, helper, foreign, flush: () => scheduled.splice(0).forEach(fn => fn()), resets: () => resets };
}

test("retires previous submissions while retaining ordinary undo within the current edit", () => {
  const f = fixture();
  for (const text of ["first", "second", "third"]) {
    f.helper.push(text, `${text} edit`);
    f.boundary.input("x");
    f.flush();
    assert.equal(f.helper.pop(), `${text} edit`);
    assert.deepEqual(f.helper, [text]);
    f.boundary.input("\r");
    assert.deepEqual(f.helper, [text], "reset waits until xterm's stack unwinds");
    f.flush();
    assert.deepEqual(f.helper, []);
  }
  assert.equal(f.resets(), 3);
  assert.deepEqual(f.foreign, ["composer draft"]);
});

test("does not reset on IME acceptance, composition end, undo, or pasted newlines", () => {
  const f = fixture();
  f.helper.push("preedit");
  f.boundary.compositionStart();
  f.boundary.input("\r");
  f.flush();
  f.boundary.compositionEnd();
  for (const data of ["確定", "\x1a", "\x1b[200~paste\r\n\x1b[201~", "\n"]) f.boundary.input(data);
  f.flush();
  assert.deepEqual(f.helper, ["preedit"]);
  assert.equal(f.resets(), 0);
  f.boundary.input("\r");
  f.flush();
  assert.deepEqual(f.helper, []);
  assert.equal(f.resets(), 1);
});

test("invalidates queued resets on new input, new composition and teardown", () => {
  for (const interrupt of ["input", "composition", "dispose"]) {
    const f = fixture();
    f.helper.push("draft");
    f.boundary.input("\r");
    if (interrupt === "input") f.boundary.input("next");
    if (interrupt === "composition") { f.boundary.compositionStart(); f.boundary.compositionEnd(); }
    if (interrupt === "dispose") f.boundary.dispose();
    f.flush();
    assert.deepEqual(f.helper, ["draft"]);
    assert.equal(f.resets(), 0);
  }
});

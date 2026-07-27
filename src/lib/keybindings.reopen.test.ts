// Run: npx tsx src/lib/keybindings.reopen.test.ts
// The reopen-closed-tab shortcut is Cmd/Ctrl+Shift+T. It must be unambiguous

import { matchesBinding } from "./keybindings";

const NEW_TAB = "Ctrl+T";
const REOPEN_TAB = "Ctrl+Shift+T";

let failed = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(e);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

// in the test runtime (matchesBinding reads metaKey on mac, ctrlKey elsewhere).
function key(code: string, shift: boolean): KeyboardEvent {
  return { code, metaKey: true, ctrlKey: true, shiftKey: shift, altKey: false, key: code } as unknown as KeyboardEvent;
}

test("Cmd/Ctrl+Shift+T matches the reopen binding", () => {
  assert(matchesBinding(key("KeyT", true), REOPEN_TAB), "shift+T should match reopen");
});

test("Cmd/Ctrl+Shift+T does NOT match new_tab", () => {
  assert(!matchesBinding(key("KeyT", true), NEW_TAB), "shift+T must not trigger new_tab");
});

test("Cmd/Ctrl+T (no shift) matches new_tab only", () => {
  assert(matchesBinding(key("KeyT", false), NEW_TAB), "plain T should match new_tab");
  assert(!matchesBinding(key("KeyT", false), REOPEN_TAB), "plain T must not match reopen");
});

console.log(`keybindings.reopen tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);

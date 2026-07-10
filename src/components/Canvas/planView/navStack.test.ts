// Run: npx tsx src/components/Canvas/planView/navStack.test.ts

import { EMPTY_NAV_STACK, NAV_STACK_MAX, navCanGoBack, navPop, navPush } from "./navStack";
import type { NavEntry } from "./navStack";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const E1: NavEntry = { surface: "task", id: "t1", planId: "p1" };
const E2: NavEntry = { surface: "plan", id: "p1", planId: "p1" };

test("empty stack: canGoBack is false", () => {
  assert(!navCanGoBack(EMPTY_NAV_STACK), "empty stack cannot go back");
  assert(EMPTY_NAV_STACK.length === 0, "empty stack has length 0");
});

test("push adds entry, canGoBack becomes true", () => {
  const s = navPush(EMPTY_NAV_STACK, E1);
  assert(s.length === 1, "length 1 after push");
  assert(s[0] === E1, "entry is E1");
  assert(navCanGoBack(s), "canGoBack after push");
});

test("push is immutable: original stack unchanged", () => {
  const s1 = navPush(EMPTY_NAV_STACK, E1);
  const s2 = navPush(s1, E2);
  assert(s1.length === 1, "s1 still has 1 entry");
  assert(s2.length === 2, "s2 has 2 entries");
});

test("pop from empty stack returns undefined entry", () => {
  const { stack, entry } = navPop(EMPTY_NAV_STACK);
  assert(entry === undefined, "no entry from empty pop");
  assert(stack.length === 0, "stack still empty");
  assert(!navCanGoBack(stack), "still cannot go back");
});

test("pop returns last entry and shrinks stack", () => {
  const s = navPush(navPush(EMPTY_NAV_STACK, E1), E2);
  const { stack, entry } = navPop(s);
  assert(entry === E2, "popped most-recent entry E2");
  assert(stack.length === 1, "stack has 1 entry remaining");
  assert(stack[0] === E1, "E1 remains in stack");
});

test("pop until empty: canGoBack becomes false", () => {
  const s = navPush(EMPTY_NAV_STACK, E1);
  const { stack } = navPop(s);
  assert(!navCanGoBack(stack), "cannot go back after popping last entry");
});

test("stack caps at NAV_STACK_MAX, oldest entries dropped", () => {
  let s = EMPTY_NAV_STACK;
  for (let i = 0; i < NAV_STACK_MAX + 5; i++) {
    s = navPush(s, { surface: "task", id: `t${i}`, planId: "p" });
  }
  assert(s.length === NAV_STACK_MAX, `stack capped at ${NAV_STACK_MAX}, got ${s.length}`);
  const { entry } = navPop(s);
  assert(entry?.id === `t${NAV_STACK_MAX + 4}`, "most recent entry at top after cap");
  assert(s[0].id === `t5`, "oldest surviving entry is correct after overflow");
});

test("reset via EMPTY_NAV_STACK clears all entries", () => {
  let s = navPush(navPush(EMPTY_NAV_STACK, E1), E2);
  assert(navCanGoBack(s), "non-empty before reset");
  s = EMPTY_NAV_STACK;
  assert(!navCanGoBack(s), "cleared after reset");
  assert(s.length === 0, "length 0 after reset");
});

test("scroll position is preserved through push/pop", () => {
  const entry: NavEntry = { surface: "task", id: "t1", planId: "p1", scroll: 420 };
  const s = navPush(EMPTY_NAV_STACK, entry);
  const { entry: popped } = navPop(s);
  assert(popped?.scroll === 420, `scroll ${popped?.scroll} !== 420`);
});

test("scroll is optional: entry without scroll pops correctly", () => {
  const entry: NavEntry = { surface: "spec", id: "p2", planId: "p2" };
  const s = navPush(EMPTY_NAV_STACK, entry);
  const { entry: popped } = navPop(s);
  assert(popped?.scroll === undefined, "scroll is undefined when not set");
  assert(popped?.surface === "spec", "surface preserved");
});

let passed = 0;
(async () => {
  for (const [name, fn] of tests) {
    try {
      fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} navStack tests passed`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});

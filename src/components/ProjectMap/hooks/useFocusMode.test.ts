import { focusReducer, focusInitial, HISTORY_LIMIT, type FocusState } from "./useFocusMode";

let passed = 0;
let failed = 0;

function eq<T>(label: string, actual: T, expected: T) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${e}, got ${a}`);
    failed++;
  }
}

console.log("focusReducer:");

const empty = focusInitial(null);
eq("initial empty", empty, { history: [], cursor: -1 });

const seed = focusInitial(1);
eq("initial seeded", seed, { history: [1], cursor: 0 });

let s: FocusState = focusInitial(null);
s = focusReducer(s, { type: "set", id: 1 });
eq("set first", s, { history: [1], cursor: 0 });

s = focusReducer(s, { type: "set", id: 2 });
s = focusReducer(s, { type: "set", id: 3 });
eq("set chain", s, { history: [1, 2, 3], cursor: 2 });

s = focusReducer(s, { type: "set", id: 3 });
eq("set duplicate is no-op", s, { history: [1, 2, 3], cursor: 2 });

s = focusReducer(s, { type: "back" });
eq("back moves cursor", s, { history: [1, 2, 3], cursor: 1 });

s = focusReducer(s, { type: "back" });
eq("back twice", s, { history: [1, 2, 3], cursor: 0 });

s = focusReducer(s, { type: "back" });
eq("back at edge stays", s, { history: [1, 2, 3], cursor: 0 });

s = focusReducer(s, { type: "forward" });
eq("forward moves cursor", s, { history: [1, 2, 3], cursor: 1 });

s = focusReducer(s, { type: "set", id: 9 });
eq("set after back trims forward", s, { history: [1, 2, 9], cursor: 2 });

s = focusReducer(s, { type: "forward" });
eq("forward at end stays", s, { history: [1, 2, 9], cursor: 2 });

let big: FocusState = focusInitial(null);
for (let i = 0; i < HISTORY_LIMIT + 5; i++) {
  big = focusReducer(big, { type: "set", id: i });
}
eq("history capped at HISTORY_LIMIT", big.history.length, HISTORY_LIMIT);
eq("history dropped oldest", big.history[0], 5);
eq("cursor at end", big.cursor, HISTORY_LIMIT - 1);

const setNullInitial: FocusState = focusReducer(focusInitial(null), { type: "set", id: null });
eq("can set null id", setNullInitial, { history: [null], cursor: 0 });

console.log(`\nResults: ${passed} passed, ${failed} failed`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}

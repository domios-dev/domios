import assert from "node:assert/strict";
import { initialStableLabel, nextStableLabel } from "./stableStatusLabel";

const feed = (start: string, labels: string[]) =>
  labels.reduce((state, label) => nextStableLabel(state, label), initialStableLabel(start)).shown;

assert.equal(feed("Working · 14m 36s", ["Working · 14m 37s", "Working · 14m 38s"]), "Working · 14m 38s", "ticking digits show at once");
assert.equal(feed("Working", ["Running tests"]), "Running tests", "a new kind of text shows at once");
assert.equal(
  feed("Working", ["Boogiening… (1m 2s)", "Working", "Boogiening… (1m 3s)", "Working", "Boogiening… (1m 5s)", "Working"]),
  "Boogiening… (1m 5s)",
  "two kinds flipping settle on the one showing",
);
assert.equal(
  feed("Running tests", ["Editing App.tsx", "Running tests", "Running tests", "Running tests"]),
  "Running tests",
  "a returning kind takes over once it clearly dominates",
);
assert.equal(feed("Running tests", ["Editing App.tsx", "Running tests", "Searching code"]), "Searching code", "a text outside the window shows at once");
assert.equal(
  feed("Leavening…", ["Working", "Leavening… (running Stop hook · 1s · ↓ 2 tokens)", "Working"]),
  "Working",
  "parenthetical detail does not make a flipping label look new",
);
assert.equal(
  ["Working", "Leavening…"].reduce((state, label) => nextStableLabel(state, label), initialStableLabel("")).shown,
  "Leavening…",
  "a provider line replaces the placeholder",
);
assert.equal(
  nextStableLabel(initialStableLabel("Leavening…"), "Working", { placeholder: true }).shown,
  "Leavening…",
  "the placeholder never replaces a provider line mid-turn",
);
assert.equal(nextStableLabel(initialStableLabel(""), "Working", { placeholder: true }).shown, "Working", "the placeholder fills an empty line");
assert.equal(
  feed("Leavening…", ["Leavening… (running Stop hook · 1s)"]),
  "Leavening… (running Stop hook · 1s)",
  "detail on the shown label updates at once",
);
assert.equal(
  feed("Running · 1s", ["Editing", "Running · 4s", "Running · 5s", "Running · 6s"]),
  "Running · 6s",
  "a returning kind keeps its latest ticking text",
);

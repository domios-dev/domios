import assert from "node:assert/strict";
import { initialStableLabel, nextStableLabel } from "./stableStatusLabel";

const feed = (start: string, labels: string[]) =>
  labels.reduce((state, label) => nextStableLabel(state, label), initialStableLabel(start)).shown;

assert.equal(feed("Working · 14m 36s", ["Working · 14m 37s", "Working · 14m 38s"]), "Working · 14m 38s", "ticking digits show at once");
assert.equal(feed("Working", ["Running tests"]), "Running tests", "a new kind of text shows at once");
assert.equal(feed("Running tests", ["Editing App.tsx", "Running tests", "Editing App.tsx"]), "Running tests", "flapping keeps the more frequent text");
assert.equal(feed("Running tests", ["Editing App.tsx", "Running tests", "Editing App.tsx", "Editing App.tsx"]), "Editing App.tsx", "the majority of the window wins");
assert.equal(feed("Running tests", ["Editing App.tsx", "Running tests", "Searching code"]), "Searching code", "a text outside the window shows at once");
assert.equal(feed("Running · 1s", ["Editing", "Running · 4s"]), "Running · 4s", "a returning kind keeps its latest ticking text");

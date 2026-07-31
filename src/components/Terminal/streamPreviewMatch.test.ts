import assert from "node:assert/strict";
import test from "node:test";

import { canonicalBlob, normalizeStreamLine, uncoveredPreviewTail } from "./streamPreviewMatch";

test("normalize strips gutter glyphs and markdown syntax", () => {
  assert.equal(normalizeStreamLine("⏺ I'll create the game."), "I'll create the game.");
  assert.equal(normalizeStreamLine("  │  **bold** and `code`  "), "bold and code");
  assert.equal(normalizeStreamLine("❯ Add a restart button"), "Add a restart button");
});

test("covered prefix is cut, uncovered tail keeps streaming", () => {
  const blob = canonicalBlob(["I'll create a complete Snake game. Canvas rendering with a grid."]);
  const preview = [
    "⏺ I'll create a complete Snake game.",
    "  Canvas rendering with a grid.",
    "  Game loop with fixed timestep", // not yet in canonical
    "  and score persistence.",
  ];
  assert.deepEqual(uncoveredPreviewTail(preview, blob), [
    "  Game loop with fixed timestep",
    "  and score persistence.",
  ]);
});

test("soft-wrapped rows match as fragments of one canonical sentence", () => {
  const blob = canonicalBlob(["A minimal but complete browser Snake game in a single self-contained file."]);
  const preview = [
    "❯ A minimal but complete browser Snake game in a single self-conta",
    "ined file.",
  ];
  assert.deepEqual(uncoveredPreviewTail(preview, blob), []);
});

test("separators and blank rows inherit neighbouring coverage", () => {
  const blob = canonicalBlob(["first paragraph", "second paragraph"]);
  const preview = ["first paragraph", "", "──", "second paragraph", "", "still streaming tail"];
  assert.deepEqual(uncoveredPreviewTail(preview, blob), ["still streaming tail"]);
});

test("nothing is cut when canonical has not landed yet", () => {
  const preview = ["⏺ Working on it", "  more text"];
  assert.deepEqual(uncoveredPreviewTail(preview, canonicalBlob([])), preview);
});

test("fully covered preview collapses to empty", () => {
  const blob = canonicalBlob(["alpha beta", "gamma delta"]);
  assert.deepEqual(uncoveredPreviewTail(["alpha beta", "gamma delta"], blob), []);
});

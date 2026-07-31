import assert from "node:assert/strict";
import test from "node:test";

import {
  canonicalBlob,
  displayablePreview,
  normalizeStreamLine,
  uncoveredPreviewTail,
} from "./streamPreviewMatch";

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

// Regression from the live codex recording: a `•` bullet row blocked the whole
// prefix cut, so the preview stayed rendered as a duplicate forever.
test("bullet-prefixed rows match their canonical text", () => {
  const blob = canonicalBlob(["Запускаю короткую поэму построчно с задержкой — хороший тест."]);
  const preview = ["• Запускаю короткую поэму построчно с задержкой — хороший тест."];
  assert.deepEqual(uncoveredPreviewTail(preview, blob), []);
});

test("tool frames and collapse markers are never previewable", () => {
  const rows = [
    "⏺ Writing the poem now",
    "│ @('line one','line two')",
    "… +6 lines (ctrl + o to expand)",
    "└ Ночь легла на холодный экран,",
    "⎿  Wrote 335 lines to snake.html",
    "+3 lines (ctrl + t to view transcript)",
    "  normal prose continues",
  ];
  assert.deepEqual(displayablePreview(rows), [
    "⏺ Writing the poem now",
    "  normal prose continues",
  ]);
});

test("volatile status rows and composer echoes are never previewable", () => {
  const rows = [
    "Working (1m 01s • esc to interrupt)",
    "ctrl + c to interrupt",
    "❯ Write a haiku",
    "› Continue",
    "────────────",
    "  real assistant prose",
  ];
  assert.deepEqual(displayablePreview(rows), ["  real assistant prose"]);
});

// Live codex probe: the "model · cwd" composer footer flapped in and out of
// the live view, shifting the whole 8-row window by one every ~250ms.
test("model/cwd composer footer is never previewable", () => {
  const rows = [
    "• applying the fix now",
    "  gpt-5.5 medium · ~/Developer/codeterm-worktrees/lane-streaming-consilium",
  ];
  assert.deepEqual(displayablePreview(rows), ["• applying the fix now"]);
});

// Live codex probe: unframed tool panes (inline diff view, CLI help output)
// alternated with message text as full-window swaps — 4Hz whole-block shake.
test("unframed diff rows and CLI help rows are never previewable", () => {
  const rows = [
    '     87 -dev_post "/api/tabs/$TAB/capture/stop" \'{}\' | tee out.json; echo',
    '    173 +dc tab capture stop --tab "$TAB" --json | tee out.json; echo',
    "    174",
    '    172  echo "-- capture stop"',
    "      -h, --help         Print help",
    "          --mesh <MESH>  Execute command on a mesh peer",
    "• I'm applying small robustness fixes to the scripts and docs now",
  ];
  assert.deepEqual(displayablePreview(rows), [
    "• I'm applying small robustness fixes to the scripts and docs now",
  ]);
});

// Live codex probe: trailing chrome (blank rows, Working status, composer
// echo) flapped against a chrome-free snapshot of the same content — after
// filtering, both snapshots must be identical so the flap is a no-op.
test("chrome-tail flap collapses to a no-op after filtering", () => {
  const content = [
    "• The first patch exposed one issue: the embedded Python question logger",
    "  cannot call the shell-only dc helper subprocess.",
  ];
  const withChrome = content.concat([
    "",
    "• Working (1m 01s • esc to interrupt)",
    "",
    "",
    "› Improve documentation in @filename",
  ]);
  assert.deepEqual(displayablePreview(withChrome), displayablePreview(content));
});

// Live codex probe: an uncovered stray row ABOVE covered content pinned the
// covered rows on screen forever (prefix-only cut) — visible dupes/leftovers.
test("an uncovered stray row above covered content no longer blocks the cut", () => {
  const blob = canonicalBlob([
    "Next I'm loading its workflow notes and then I'll claim/read the task details.",
  ]);
  const preview = [
    "some stray tool output row that never lands in canon",
    "Next I'm loading its workflow notes and then I'll claim/read the task details.",
    "fresh streaming tail keeps going",
  ];
  assert.deepEqual(uncoveredPreviewTail(preview, blob), ["fresh streaming tail keeps going"]);
});

// Live codex probe: an ultra-short answer ("ok3") was consumed as a
// non-decisive row before any canon existed, so short turns never previewed.
test("ultra-short answer with no canon yet still previews", () => {
  assert.deepEqual(uncoveredPreviewTail(["• ok3"], canonicalBlob([])), ["• ok3"]);
});

test("short trailing rows are still consumed after a real covered cut", () => {
  const blob = canonicalBlob(["first paragraph landed in canon"]);
  const preview = ["first paragraph landed in canon", "", "next long streaming row"];
  assert.deepEqual(uncoveredPreviewTail(preview, blob), ["next long streaming row"]);
});

// Live opencode recording: the mode/model status badge and the reasoning
// duration row flapped in the live region and never land in canon.
test("status badge and reasoning-duration rows are never previewable", () => {
  const rows = [
    "     ▣  Build · DeepSeek V4 Flash Free (New)",
    "     + Thought: 304ms",
    "     Elapsed: 1.2s",
    '     A PTY (pseudo-terminal) is a kernel-emulated virtual terminal pairing a "master" process',
  ];
  assert.deepEqual(displayablePreview(rows), [
    '     A PTY (pseudo-terminal) is a kernel-emulated virtual terminal pairing a "master" process',
  ]);
});

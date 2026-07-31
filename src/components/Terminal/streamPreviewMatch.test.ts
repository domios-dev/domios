import assert from "node:assert/strict";
import test from "node:test";

import {
  canonicalBlob,
  capPreviewRows,
  createPreviewStabilizerState,
  displayablePreview,
  isSmoothPreviewTransition,
  normalizeStreamLine,
  stabilizePreview,
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

// eight main ruling: tool-run blocks belong to canonical tool cards, never
// the preview. Headers morph in place ('Exploring'->'Explored'), children
// collapse/expand — structurally unfixable as smooth preview content.
test("tool-run headers, diffstats, and tool child rows are never previewable", () => {
  const rows = [
    "• Explored",
    "• Exploring",
    "• Added streaming-probe.txt (+1 -0)",
    "    Search render_timeline|timeline-motion in .",
    "    wrote captures/streaming/opencode/timeline-motion.json (57 frames)",
    "• The scorer runs on the Codex capture and shows the intended improvement",
    "  because its motion timeline has not been rendered yet.",
    "• def fizzbuzz(n):",
    "      return [",
    "     A PTY (pseudo-terminal) is a kernel-emulated virtual terminal",
  ];
  assert.deepEqual(displayablePreview(rows), [
    "• The scorer runs on the Codex capture and shows the intended improvement",
    "  because its motion timeline has not been rendered yet.",
    "• def fizzbuzz(n):",
    "      return [",
    "     A PTY (pseudo-terminal) is a kernel-emulated virtual terminal",
  ]);
});

// Rogue-probe heavy-tool corpus: when the decoder's live slice shrinks, the
// 8-row window slid BACKWARD, re-showing rows already cut from the top.
test("stabilizer: rows cut from the top never re-enter the window", () => {
  const state = createPreviewStabilizerState();
  assert.deepEqual(stabilizePreview(state, 1, ["b", "c", "d"]), ["b", "c", "d"]);
  assert.deepEqual(stabilizePreview(state, 1, ["c", "d", "e"]), ["c", "d", "e"]);
  assert.deepEqual(stabilizePreview(state, 1, ["b", "c", "d", "e"]), ["c", "d", "e"]);
});

test("stabilizer: transient bottom collapse holds the current display", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["a", "b", "c"]);
  assert.deepEqual(stabilizePreview(state, 1, ["a"]), ["a", "b", "c"]);
  assert.deepEqual(stabilizePreview(state, 1, ["a", "b", "c", "d"]), ["a", "b", "c", "d"]);
});

test("stabilizer: empty input (coverage death) always passes through", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["a", "b"]);
  assert.deepEqual(stabilizePreview(state, 1, []), []);
});

test("stabilizer: new turn resets retirement", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["a", "b"]);
  stabilizePreview(state, 1, ["b"]);
  assert.deepEqual(stabilizePreview(state, 2, ["a", "b"]), ["a", "b"]);
});

test("keyboard-hint rows are never previewable", () => {
  const rows = ["  esc again to edit previous message", "  Escaping the loop requires a break statement."];
  assert.deepEqual(displayablePreview(rows), ["  Escaping the loop requires a break statement."]);
});

// Live opencode run: a block-element border row flapped against the braille
// spinner as 1-row full swaps; queued prompts pin as "↳ …" + "(…)" footers
// while answer rows insert above them; codex warm-up shows system banners.
test("border, spinner, queue-footer, and banner rows are never previewable", () => {
  const rows = [
    "  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀",
    "     ⠋ Thinking",
    "     ⠙ Thinking",
    "  ↳ Write a python function fizzbuzz(n) that returns a list of strings.",
    "  (… immediately)",
    "Tip: New Use /fast to toggle fast mode.",
    "• You have 1 usage limit reset available. Run /usage to use one.",
    "  A queue is a FIFO data structure (first in, first out).",
  ];
  assert.deepEqual(displayablePreview(rows), [
    "  A queue is a FIFO data structure (first in, first out).",
  ]);
});

// Required interactive-prompt coverage: a genuine built-in question UI is
// assistant CONTENT — the question and its option rows must never be
// filtered as composer chrome, while a plain echoed draft still is.
test("interactive question and option rows survive the chrome filters", () => {
  const rows = [
    "⏺ Which database should the new service use?",
    "❯ 1. PostgreSQL (recommended)",
    "  2. SQLite",
    "› 3. MySQL",
    "› keep the old one for now",
  ];
  assert.deepEqual(displayablePreview(rows), [
    "⏺ Which database should the new service use?",
    "❯ 1. PostgreSQL (recommended)",
    "  2. SQLite",
    "› 3. MySQL",
  ]);
});

// Live opencode long-prose turn: the TUI re-emits the wrapped paragraph with
// new break points, pushing shown rows DOWN (insert-above) or replacing every
// row string. Shown rows must keep their position; a full rewrap holds.
test("stabilizer: paragraph rewrap re-anchors instead of inserting above", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["screen. When a terminal emulator receives more", "lines than fit, it stores them."]);
  assert.deepEqual(
    stabilizePreview(state, 1, [
      "Terminal scrollback is the mechanism by which",
      "screen. When a terminal emulator receives more",
      "lines than fit, it stores them.",
      "New appended row streams below.",
    ]),
    [
      "screen. When a terminal emulator receives more",
      "lines than fit, it stores them.",
      "New appended row streams below.",
    ],
  );
});

test("stabilizer: full rewrap with no surviving row holds the display", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["screen. When a terminal emulator receives", "more lines than fit, it stores them."]);
  assert.deepEqual(
    stabilizePreview(state, 1, ["screen. When a terminal", "emulator receives more lines", "than fit, it stores them. And"]),
    ["screen. When a terminal emulator receives", "more lines than fit, it stores them."],
  );
});

// Live opencode: '~ Preparing write...' morphing to '← Write file' is a
// tool-status row, not message content.
test("tool-status glyph rows are never previewable", () => {
  const rows = ["     ~ Preparing write...", "     ← Write streaming-probe.txt", "     Real prose stays."];
  assert.deepEqual(displayablePreview(rows), ["     Real prose stays."]);
});

// Live opencode: the status bar says "esc interrupt" (no "to") and leads with
// progress dots — both must be chrome.
test("progress-dot status bars are never previewable", () => {
  const rows = [
    "   ⬝⬝⬝⬝⬝⬝■■  esc interrupt                    23.6K (12%)  ctrl+p commands",
    "     esc interrupt",
    "     real prose row survives here",
  ];
  assert.deepEqual(displayablePreview(rows), ["     real prose row survives here"]);
});

// Final opencode capture, turn 3: the wrapped cwd/token/keys footer is a
// column-aligned row — internal 6+ space runs never occur in wrapped prose.
test("column-aligned footer rows are never previewable", () => {
  const rows = [
    "   /private/tmp/claude-502/scratchpad/probe-opencode-         21.3K (11 ctrl+p",
    "   consilium/501ecdcd/scratchpad/probe-opencode                              commands",
    "   Prose keeps single spaces when it wraps across rows like this one does.",
  ];
  assert.deepEqual(displayablePreview(rows), [
    "   Prose keeps single spaces when it wraps across rows like this one does.",
  ]);
});

// Final opencode capture, turn 9 (long-prose): mid-window rewrap changed a
// row ABOVE the streaming tail in place — hold; tail growth still flows.
test("stabilizer: mid-window rewrap holds, tail growth passes", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["first shown row stays", "second shown row here", "third streaming tail"]);
  assert.deepEqual(
    stabilizePreview(state, 1, ["first shown row stays", "second shown row HERE rewrapped", "third streaming tail plus"]),
    ["first shown row stays", "second shown row here", "third streaming tail"],
  );
  assert.deepEqual(
    stabilizePreview(state, 1, ["first shown row stays", "second shown row here", "third streaming tail plus more"]),
    ["first shown row stays", "second shown row here", "third streaming tail plus more"],
  );
});

// Claude capture: the completed-turn verb row ("✻ Brewed for 2s") and
// line-numbered file dumps leaked as leftovers.
test("completed-verb and numbered file rows are never previewable", () => {
  const rows = ["✻ Brewed for 2s", "✻ Sautéed for 16s", "      1 hello streaming", "  Real prose."];
  assert.deepEqual(displayablePreview(rows), ["  Real prose."]);
});

// Claude capture: post-turn frames are all-chrome, so the hold kept covered
// content alive for minutes. A hold must respect coverage: held rows already
// in canon die even while holding.
test("stabilizer: a hold never outlives canonical coverage", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["the answer paragraph is here", "still streaming more"], true, "");
  const blob = canonicalBlob(["the answer paragraph is here still streaming more"]);
  assert.deepEqual(stabilizePreview(state, 1, [], false, blob), []);
});

test("stabilizer: transient all-chrome frame still holds uncovered content", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["fresh uncovered content row"], true, "");
  assert.deepEqual(stabilizePreview(state, 1, [], false, ""), ["fresh uncovered content row"]);
});

// Required interactive-prompt coverage (opencode): the question dialog is
// ┃-framed like tool output, but a framed run containing checkbox/radio
// option rows is a genuine question the user must see — kept wholesale,
// minus key-glyph hint rows. Framed runs WITHOUT options stay tool junk.
test("framed interactive question dialogs survive; framed tool output does not", () => {
  const dialog = [
    "┃  How was this opencode-in-CodeTerm probe session? Select all that apply.",
    "┃  1. [ ] Smooth",
    "┃     Everything worked as expected",
    "┃  2. [x] Confusing",
    "┃  ⇆ tab ↑↓ select enter toggle esc dismiss",
  ];
  assert.deepEqual(displayablePreview(dialog), [
    "┃  How was this opencode-in-CodeTerm probe session? Select all that apply.",
    "┃  1. [ ] Smooth",
    "┃     Everything worked as expected",
    "┃  2. [x] Confusing",
  ]);
  const toolFrame = ["┃ $ cargo build --release", "┃ Compiling codeterm v1.7.8", "  prose survives"];
  assert.deepEqual(displayablePreview(toolFrame), ["  prose survives"]);
});

// Policy ruling: a question dialog taller than the cap anchors the window at
// its question row — options without their question are unreadable. Plain
// streaming content keeps last-rows semantics.
test("cap anchors at the question row for tall dialogs", () => {
  const dialog = [
    "earlier prose row",
    "┃  How was this probe session? Select all that apply.",
    "┃  1. [ ] Smooth",
    "┃     Everything worked",
    "┃  2. [ ] Confusing",
    "┃     Setup was unclear",
    "┃  3. [ ] Slow",
    "┃     Long waits",
    "┃  4. [ ] Broken",
    "┃     Errors everywhere",
    "┃  5. [ ] Other",
  ];
  const capped = capPreviewRows(dialog);
  assert.equal(capped.length, 8);
  assert.equal(capped[0], "┃  How was this probe session? Select all that apply.");
  const prose = Array.from({ length: 12 }, (_, i) => `prose row ${i}`);
  assert.deepEqual(capPreviewRows(prose), prose.slice(-8));
});

test("question dialog open and close are deliberate modal transitions", () => {
  const prose = ["streaming prose one", "streaming prose two"];
  const dialog = ["┃  Pick one now please", "┃  1. [ ] Yes", "┃  2. [ ] No"];
  assert.equal(isSmoothPreviewTransition(prose, dialog), true);
  assert.equal(isSmoothPreviewTransition(dialog, prose), true);
});

// Claude re-record: spinner verbs can be hyphenated compounds.
test("hyphenated spinner verbs are never previewable", () => {
  const rows = [
    "✳ Sock-hopping…",
    "✢ Fiddle-faddling…",
    "✳ Fiddle-faddling… (1s · thinking with high effort)",
    "  Actual content survives.",
  ];
  assert.deepEqual(displayablePreview(rows), ["  Actual content survives."]);
});

// ux-verifier nit: option rows retired from pre-anchor frames were trimmed
// out of the anchored dialog window. Dialog frames bypass retirement.
test("stabilizer: retired rows still display inside a question dialog", () => {
  const state = createPreviewStabilizerState();
  stabilizePreview(state, 1, ["┃  1. [ ] Smooth", "some other row"], true, "");
  stabilizePreview(state, 1, ["some other row"], true, "");
  assert.deepEqual(
    stabilizePreview(state, 1, ["┃  Pick one please", "┃  1. [ ] Smooth", "┃  2. [ ] Confusing"], true, ""),
    ["┃  Pick one please", "┃  1. [ ] Smooth", "┃  2. [ ] Confusing"],
  );
});

// Answered opencode round-trip: the SINGLE-SELECT picker has no checkbox
// markers — plain numbered option rows. Two-plus numbered rows in a framed
// run are a dialog; a lone framed numbered row stays tool junk.
test("single-select numbered dialogs survive; lone framed numbered rows do not", () => {
  const dialog = [
    "  ┃  What would you like me to do next?",
    "  ┃",
    "  ┃  1. Wrap up",
    "  ┃     Conclude the probe and report final status",
    "  ┃  2. Investigate",
  ];
  assert.deepEqual(displayablePreview(dialog), dialog);
  const toolish = ["┃ 1. compiling codeterm", "┃ done in 3s", "  prose survives"];
  assert.deepEqual(displayablePreview(toolish), ["  prose survives"]);
});

// Answered round-trip, option-2 nit: once a PARTIAL dialog frame was
// accepted, the complete frame was a mid-window insert — held forever.
// Dialog views track the decoder verbatim: internal redraws always accepted.
test("stabilizer: a dialog completing mid-window is accepted verbatim", () => {
  const state = createPreviewStabilizerState();
  const partial = [
    "┃  What would you like me to do next?",
    "┃  1. Wrap up",
    "┃     Conclude the probe and report final status",
    "┃     Investigate further before concluding",
  ];
  const complete = [
    "┃  What would you like me to do next?",
    "┃  1. Wrap up",
    "┃     Conclude the probe and report final status",
    "┃  2. Investigate",
    "┃     Investigate further before concluding",
  ];
  stabilizePreview(state, 1, partial, true, "");
  assert.deepEqual(stabilizePreview(state, 1, complete, true, ""), complete);
});

// Wide-140 geometry leaks (per ruling, all client chrome): apostrophe-elided
// gerund spinners, the client feedback survey, border+fragment composite
// rows, right-aligned banners, and wrapped tails of dropped ⎿/Tip rows.
test("wide-geometry chrome composites are never previewable", () => {
  const rows = [
    "· Beboppin'…",
    "✶ Beboppin'… (1s · ↓ 1 tokens)",
    "● How is Claude doing this session? (optional)",
    "  1: Bad    2: Fine   3: Good   0: Dismiss",
    "  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀    ad6a-4126-a64c-a27f7643937c/",
    "                                                                                                OpenCode includes free models",
    "  Real assistant prose stays visible.",
  ];
  assert.deepEqual(displayablePreview(rows), ["  Real assistant prose stays visible."]);
});

test("wrapped tail of a dropped framed Tip row dies with its head", () => {
  const rows = [
    "  ⎿ Tip: Dynamic workflows let Claude write a script that orchestrates many agents. Mention the keyword ultraco",
    "     use a workflow directly.",
    "  Normal prose continues here.",
  ];
  assert.deepEqual(displayablePreview(rows), ["  Normal prose continues here."]);
});

// opencode-narrow80: the 80-col footer wraps differently — the token counter
// loses its space ('(14ctrl+p') and the cwd continues as a bare path row.
test("narrow-geometry footer wraps are never previewable", () => {
  const rows = [
    "   /private/tmp/claude-502/-Users-dev-Developer-     27.7K (14ctrl+p",
    "   ad6a-4126-a64c-a27f7643937c/scratchpad/probe-opencode",
    "   A PTY is a kernel object that pairs a master and a slave device.",
  ];
  assert.deepEqual(displayablePreview(rows), [
    "   A PTY is a kernel object that pairs a master and a slave device.",
  ]);
});

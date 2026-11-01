import assert from "node:assert/strict";
import { test } from "node:test";
import { chatScrollBookmark, chatScrollRestoreTarget } from "./chatScrollMemory";
import { chatAnchorShift, chatShiftAnchorAt, chatSpacerAfterLayout, chatSpacerDriftAfterShift, chatSpacerFold, chatSpacerHeight, shouldAbsorbChatShift,
  chatSpacerTranslation, unrecordedRowsAbove } from "./chatSpacerDrift";

test("a row above the anchor that grew before its resize notification landed is still owed its shift", () => {
  // A command card above the view grew 420px while the finger moved and new rows arrived below; its notification was
  // deferred a frame, and it now reaches into the view itself. Every row up to the previous anchor is checked.
  const rows = [{ id: "a", top: 0 }, { id: "card", top: 100 }, { id: "anchor", top: 840 }, { id: "below", top: 1200 }];
  const measured = [100, 740, 360, 200];
  const recorded = new Map([["a", 100], ["card", 320], ["anchor", 360], ["below", 140]]);
  assert.deepEqual(unrecordedRowsAbove(rows, measured, 2, recorded), [1], "the card's growth is pending, the row below the view is not");
  assert.deepEqual(unrecordedRowsAbove(rows, measured, 1, recorded), [], "only rows above the given anchor count");
  assert.deepEqual(unrecordedRowsAbove(rows, [100, 320, 360, 200], 2, recorded), [], "recorded heights owe nothing");
  assert.deepEqual(unrecordedRowsAbove(rows, measured, 2, new Map()), [], "a row never measured is placed by its first measurement, not here");
});

test("absorbs a detached owned scroll on every engine", () => {
  assert.equal(shouldAbsorbChatShift({ pinned: false, userScrolling: true }), true);
  assert.equal(shouldAbsorbChatShift({ pinned: true, userScrolling: true }), false);
  assert.equal(shouldAbsorbChatShift({ pinned: false, userScrolling: false }), false);
});

test("activation with stick-to-bottom ignores the drift hold", () => {
  const bookmark = chatScrollBookmark({ mode: "pinned", scrollTop: 0 });
  const pinned = bookmark.mode === "pinned";
  assert.equal(shouldAbsorbChatShift({ pinned, userScrolling: true }), false);
  const restored = chatScrollRestoreTarget(bookmark, 0, 24000 - 800);
  assert.deepEqual(restored, { mode: "pinned", top: 23200 });
  assert.equal(24000 - restored.top - 800, 0);
});

test("drift cancels a shift above the anchor so the anchor keeps its content offset", () => {
  const drift = chatSpacerDriftAfterShift(0, 1000, 240);
  assert.equal(drift, -240);
  assert.equal(chatSpacerHeight(1000, drift) + 240, 1000);
});

test("drift grows the spacer when mounted rows are shorter than their estimate", () => {
  assert.equal(chatSpacerDriftAfterShift(-40, 1000, -300), 260);
});

test("drift beyond the spacer is carried as a row translation", () => {
  assert.equal(chatSpacerDriftAfterShift(-100, 300, 500), -600);
  assert.equal(chatSpacerHeight(300, -600), 0);
  assert.equal(chatSpacerTranslation(300, -600), -300);
});

test("drift is kept when nothing moved or the shift is unknown", () => {
  assert.equal(chatSpacerDriftAfterShift(30, 1000, 0), 30);
  assert.equal(chatSpacerDriftAfterShift(30, 1000, Number.NaN), 30);
});

test("fold returns the scroll delta that offsets removing the drift", () => {
  assert.deepEqual(chatSpacerFold(1000, -240), { drift: 0, scrollBy: 240 });
  assert.deepEqual(chatSpacerFold(1000, 300), { drift: 0, scrollBy: -300 });
});

test("fold includes the translated remainder beyond an exhausted spacer", () => {
  assert.deepEqual(chatSpacerFold(100, -400), { drift: 0, scrollBy: 400 });
});

test("ordinary drift needs no row translation", () => {
  assert.equal(chatSpacerTranslation(18000, -16888), 0);
  assert.equal(chatSpacerTranslation(0, 300), 0);
});

test("a layout pass absorbs an owned shift while the spacer has room", () => {
  assert.deepEqual(chatSpacerAfterLayout({ base: 20000, drift: 0, shift: 300, absorb: true, viewportHeight: 800, scrollTop: 30000 }), { drift: -300, scrollBy: 0 });
});

test("a layout pass leaves a shift it does not own to the idle writer", () => {
  assert.deepEqual(chatSpacerAfterLayout({ base: 20000, drift: -100, shift: 300, absorb: false, viewportHeight: 800, scrollTop: 30000 }), { drift: -100, scrollBy: 0 });
});

test("a shift larger than the spacer stays held until the gesture ends", () => {
  // Spacer holds 2822px; a fling mounts rows 3600px taller than estimated.
  const base = 18960;
  const drift = 2822 - base;
  const next = chatSpacerAfterLayout({ base, drift, shift: 3600, absorb: true, viewportHeight: 844, scrollTop: 3000 });
  assert.equal(next.drift, drift - 3600);
  // Content above the anchor grows by the shift plus the restored spacer; the scroll follows it exactly.
  assert.equal(next.scrollBy, 0);
  assert.equal(chatSpacerFold(base, next.drift).scrollBy, 3600 + base - 2822);
});

test("an idle layout pass preserves drift until the gesture-end fold owns the write", () => {
  assert.deepEqual(chatSpacerAfterLayout({ base: 0, drift: 400, shift: 0, absorb: false, viewportHeight: 800, scrollTop: 900 }), { drift: 400, scrollBy: 0 });
});

test("a shorter first page is held above the first row without a mid-gesture fold", () => {
  // A fling sits 117px into estimated rows that mount 286px shorter; the anchor would need scrollTop -169.
  assert.deepEqual(chatSpacerAfterLayout({ base: 0, drift: 0, shift: -286, absorb: true, viewportHeight: 844, scrollTop: 117 }),
    { drift: 286, scrollBy: 0 });
  // The held space stays put while the finger rests at the top, and folds once the view moves down far enough.
  assert.deepEqual(chatSpacerAfterLayout({ base: 0, drift: 169, shift: 0, absorb: true, viewportHeight: 844, scrollTop: 0 }),
    { drift: 169, scrollBy: 0 });
  assert.deepEqual(chatSpacerAfterLayout({ base: 0, drift: 169, shift: 0, absorb: true, viewportHeight: 844, scrollTop: 400 }),
    { drift: 169, scrollBy: 0 });
});

test("the shift anchor is the first row reaching into the viewport", () => {
  const rows = [{ id: "a", top: 0 }, { id: "b", top: 100 }, { id: "c", top: 400 }];
  const heights = [100, 300, 50];
  assert.equal(chatShiftAnchorAt(rows, heights, 0)?.id, "a");
  assert.equal(chatShiftAnchorAt(rows, heights, 100)?.id, "b");
  assert.equal(chatShiftAnchorAt(rows, heights, 399)?.id, "b");
  assert.equal(chatShiftAnchorAt(rows, heights, 9000)?.id, "c");
  assert.equal(chatShiftAnchorAt([], [], 0), null);
});

test("anchor shift measures how far content above moved the anchor", () => {
  assert.equal(chatAnchorShift({ id: "a", top: 500 }, { id: "a", top: 620 }), 120);
  assert.equal(chatAnchorShift(null, { id: "a", top: 620 }), 0);
  assert.equal(chatAnchorShift({ id: "a", top: 500 }, null), 0);
});

test("a day separator leaving the anchor row for a prepended page is absorbed as a shift of its body", () => {
  const before = { id: "a", top: 500, contentTop: 528 };
  const afterPrepend = { id: "a", top: 900, contentTop: 900 };
  assert.equal(chatAnchorShift(before, afterPrepend), 372);
});

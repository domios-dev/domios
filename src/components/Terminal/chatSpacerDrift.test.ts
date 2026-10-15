import assert from "node:assert/strict";
import { test } from "node:test";
import { chatAnchorShift, chatShiftAnchorAt, chatSpacerAfterLayout, chatSpacerDriftAfterShift, chatSpacerFold, chatSpacerHeight, shouldAbsorbChatShift,
  shouldFoldChatSpacerDrift } from "./chatSpacerDrift";

test("absorbs only a detached user scroll on an engine without scroll anchoring", () => {
  assert.equal(shouldAbsorbChatShift({ nativeAnchoring: false, pinned: false, userScrolling: true }), true);
  assert.equal(shouldAbsorbChatShift({ nativeAnchoring: true, pinned: false, userScrolling: true }), false);
  assert.equal(shouldAbsorbChatShift({ nativeAnchoring: false, pinned: true, userScrolling: true }), false);
  assert.equal(shouldAbsorbChatShift({ nativeAnchoring: false, pinned: false, userScrolling: false }), false);
});

test("drift cancels a shift above the anchor so the anchor keeps its content offset", () => {
  const drift = chatSpacerDriftAfterShift(0, 1000, 240);
  assert.equal(drift, -240);
  assert.equal(chatSpacerHeight(1000, drift) + 240, 1000);
});

test("drift grows the spacer when mounted rows are shorter than their estimate", () => {
  assert.equal(chatSpacerDriftAfterShift(-40, 1000, -300), 260);
});

test("drift never shrinks the spacer below zero", () => {
  assert.equal(chatSpacerDriftAfterShift(-100, 300, 500), -300);
  assert.equal(chatSpacerHeight(300, -300), 0);
});

test("drift is kept when nothing moved or the shift is unknown", () => {
  assert.equal(chatSpacerDriftAfterShift(30, 1000, 0), 30);
  assert.equal(chatSpacerDriftAfterShift(30, 1000, Number.NaN), 30);
});

test("fold returns the scroll delta that offsets removing the drift", () => {
  assert.deepEqual(chatSpacerFold(1000, -240), { drift: 0, scrollBy: 240 });
  assert.deepEqual(chatSpacerFold(1000, 300), { drift: 0, scrollBy: -300 });
});

test("fold counts only the part of a negative drift the spacer actually held", () => {
  assert.deepEqual(chatSpacerFold(100, -400), { drift: 0, scrollBy: 100 });
});

test("drift folds mid-gesture only once the spacer nears exhaustion or the transcript top", () => {
  assert.equal(shouldFoldChatSpacerDrift(18000, -16000, 800), false);
  assert.equal(shouldFoldChatSpacerDrift(18000, -16888, 800), true);
  assert.equal(shouldFoldChatSpacerDrift(0, 300, 800), true);
  assert.equal(shouldFoldChatSpacerDrift(500, 0, 800), false);
  assert.equal(shouldFoldChatSpacerDrift(5000, 400, 800), false);
});

test("a layout pass absorbs an owned shift while the spacer has room", () => {
  assert.deepEqual(chatSpacerAfterLayout({ base: 20000, drift: 0, shift: 300, absorb: true, viewportHeight: 800, scrollTop: 30000 }), { drift: -300, scrollBy: 0 });
});

test("a layout pass leaves a shift it does not own to the idle writer", () => {
  assert.deepEqual(chatSpacerAfterLayout({ base: 20000, drift: -100, shift: 300, absorb: false, viewportHeight: 800, scrollTop: 30000 }), { drift: -100, scrollBy: 0 });
});

test("a shift larger than the spacer folds whole into one scroll delta", () => {
  // Spacer holds 2822px; a fling mounts rows 3600px taller than estimated.
  const base = 18960;
  const drift = 2822 - base;
  const next = chatSpacerAfterLayout({ base, drift, shift: 3600, absorb: true, viewportHeight: 844, scrollTop: 3000 });
  assert.equal(next.drift, 0);
  // Content above the anchor grows by the shift plus the restored spacer; the scroll follows it exactly.
  assert.equal(next.scrollBy, 3600 + base - 2822);
});

test("an idle fold at the transcript top only removes the drift", () => {
  assert.deepEqual(chatSpacerAfterLayout({ base: 0, drift: 400, shift: 0, absorb: false, viewportHeight: 800, scrollTop: 900 }), { drift: 0, scrollBy: -400 });
});

test("a mid-gesture fold at the transcript top keeps what it cannot scroll as space above the first row", () => {
  // A fling sits 117px into estimated rows that mount 286px shorter; the anchor would need scrollTop -169.
  assert.deepEqual(chatSpacerAfterLayout({ base: 0, drift: 0, shift: -286, absorb: true, viewportHeight: 844, scrollTop: 117 }),
    { drift: 169, scrollBy: -117 });
  // The held space stays put while the finger rests at the top, and folds once the view moves down far enough.
  assert.deepEqual(chatSpacerAfterLayout({ base: 0, drift: 169, shift: 0, absorb: true, viewportHeight: 844, scrollTop: 0 }),
    { drift: 169, scrollBy: 0 });
  assert.deepEqual(chatSpacerAfterLayout({ base: 0, drift: 169, shift: 0, absorb: true, viewportHeight: 844, scrollTop: 400 }),
    { drift: 0, scrollBy: -169 });
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
  assert.equal(chatAnchorShift({ id: "a", top: 500 }, 620), 120);
  assert.equal(chatAnchorShift(null, 620), 0);
  assert.equal(chatAnchorShift({ id: "a", top: 500 }, null), 0);
});

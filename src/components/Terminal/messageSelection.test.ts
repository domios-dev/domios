import { describe, expect, it } from "vitest";
import { EMPTY_SELECTION, orderedSelection, reduceMessageSelection, selectionActive, type MessageSelection, type MessageSelectionAction } from "./messageSelection";

const ORDER = ["a", "b", "c", "d", "e"];
const run = (...actions: MessageSelectionAction[]): MessageSelection => actions.reduce(reduceMessageSelection, EMPTY_SELECTION);
const toggle = (id: string): MessageSelectionAction => ({ type: "toggle", id });
const extend = (id: string): MessageSelectionAction => ({ type: "extend", id, order: ORDER });
const move = (delta: 1 | -1, shift = false): MessageSelectionAction => ({ type: "move", delta, order: ORDER, extend: shift });

describe("reduceMessageSelection", () => {
  it("toggle adds and removes, and an empty selection is inactive", () => {
    const one = run(toggle("b"));
    expect(one.selected).toEqual(["b"]);
    expect(selectionActive(one)).toBe(true);
    expect(run(toggle("b"), toggle("b"))).toBe(EMPTY_SELECTION);
  });

  it("extend selects the range from the anchor and keeps earlier picks", () => {
    const state = run(toggle("a"), toggle("c"), extend("e"));
    expect(state.selected).toEqual(["a", "c", "d", "e"]);
    expect(run(toggle("d"), extend("b")).selected).toEqual(["b", "c", "d"]);
  });

  it("extend without an anchor selects just the target", () => {
    expect(run(extend("c")).selected).toEqual(["c"]);
  });

  it("shift+arrows grow then shrink the range around the anchor", () => {
    const grown = run(toggle("b"), move(1, true), move(1, true));
    expect(grown.selected).toEqual(["b", "c", "d"]);
    expect(reduceMessageSelection(grown, move(-1, true)).selected).toEqual(["b", "c"]);
  });

  it("shift+arrow from a focused row anchors at that row", () => {
    expect(run({ type: "focus", id: "c" }, move(1, true)).selected).toEqual(["c", "d"]);
  });

  it("plain arrows move the cursor only and clamp at the ends", () => {
    const state = run(toggle("a"), move(-1), move(1), move(1));
    expect(state.selected).toEqual(["a"]);
    expect(state.cursor).toBe("c");
    expect(run(move(1)).cursor).toBe("a");
    expect(run(move(-1)).cursor).toBe("e");
    expect(run({ type: "focus", id: "e" }, move(1)).cursor).toBe("e");
  });

  it("prune drops vanished rows and clears when none remain", () => {
    const state = run(toggle("a"), toggle("c"));
    expect(reduceMessageSelection(state, { type: "prune", order: ["c", "d"] }).selected).toEqual(["c"]);
    expect(reduceMessageSelection(state, { type: "prune", order: ["x"] })).toBe(EMPTY_SELECTION);
    expect(reduceMessageSelection(state, { type: "prune", order: ORDER })).toBe(state);
  });

  it("a drag selects whole rows from its start to the pointer and shrinks when the pointer comes back", () => {
    const start = (id: string, extend = false): MessageSelectionAction => ({ type: "dragStart", id, order: ORDER, extend });
    const to = (id: string): MessageSelectionAction => ({ type: "dragTo", id, order: ORDER });
    const dragging = run(start("b"), to("d"));
    expect(dragging.selected).toEqual(["b", "c", "d"]);
    expect(selectionActive(dragging)).toBe(true);
    expect(reduceMessageSelection(dragging, to("c")).selected).toEqual(["b", "c"]);
    expect(reduceMessageSelection(dragging, to("a")).selected).toEqual(["a", "b"]);
    const done = reduceMessageSelection(dragging, { type: "dragEnd" });
    expect(done.drag).toBeNull();
    expect(done.base).toEqual(["b", "c", "d"]);
  });

  it("a drag keeps earlier picks, and one starting on a selected row removes instead", () => {
    const start = (id: string): MessageSelectionAction => ({ type: "dragStart", id, order: ORDER, extend: false });
    const to = (id: string): MessageSelectionAction => ({ type: "dragTo", id, order: ORDER });
    expect(run(toggle("a"), start("c"), to("d"), { type: "dragEnd" }).selected).toEqual(["a", "c", "d"]);
    expect(run(toggle("a"), toggle("b"), toggle("c"), start("b"), to("c"), { type: "dragEnd" }).selected).toEqual(["a"]);
    expect(run(toggle("b"), start("b"), { type: "dragEnd" })).toBe(EMPTY_SELECTION);
  });

  it("a press without a drag toggles its row, and shift extends from the anchor", () => {
    const press = (id: string, extend = false): MessageSelectionAction[] => [{ type: "dragStart", id, order: ORDER, extend }, { type: "dragEnd" }];
    expect(run(...press("c")).selected).toEqual(["c"]);
    expect(run(...press("b"), ...press("d", true)).selected).toEqual(["b", "c", "d"]);
    expect(run({ type: "dragTo", id: "c", order: ORDER })).toBe(EMPTY_SELECTION);
  });

  it("clear resets everything", () => {
    expect(run(toggle("a"), { type: "clear" })).toBe(EMPTY_SELECTION);
  });

  it("orderedSelection follows transcript order, not pick order", () => {
    expect(orderedSelection(run(toggle("d"), toggle("a"), toggle("c")), ORDER)).toEqual(["a", "c", "d"]);
  });
});


it("counts partial selections as parts and restores a whole message when all its parts are selected", async () => {
  const { indexMessageParts, clipsForParts, partSelectionSummary } = await import("./messagePartSelection");
  const index = indexMessageParts([
    { id: "m", role: "assistant", author: "Agent", source: "  First.\n\nSecond.\n" },
    { id: "n", role: "assistant", author: "Agent", source: "Next." },
  ]);
  const [first, second, next] = index.order;
  const partial = partSelectionSummary(index, [second, next]);
  expect(partial.partial).toBe(true);
  expect(partial.count).toBe(2);
  expect(clipsForParts(index, [second, next]).map((clip) => clip.source)).toEqual(["Second.", "Next."]);
  const whole = partSelectionSummary(index, [first, second, next]);
  expect(whole.partial).toBe(false);
  expect(whole.count).toBe(2);
  expect(clipsForParts(index, [first, second])[0].source).toBe("  First.\n\nSecond.\n");
  expect(clipsForParts(index, ["missing"])).toEqual([]);
}, 20_000);

it("adds and removes a contiguous range of source part IDs across message boundaries", () => {
  const order = ["a:0-10", "a:12-20", "b:0-6", "b:8-14"];
  const start = reduceMessageSelection(EMPTY_SELECTION, { type: "dragStart", id: order[1], order, extend: false });
  const added = reduceMessageSelection(start, { type: "dragTo", id: order[3], order });
  expect(orderedSelection(added, order)).toEqual(["a:12-20", "b:0-6", "b:8-14"]);
  const ended = reduceMessageSelection(added, { type: "dragEnd" });
  const removing = reduceMessageSelection(ended, { type: "dragStart", id: order[3], order, extend: false });
  const removed = reduceMessageSelection(removing, { type: "dragTo", id: order[2], order });
  expect(orderedSelection(removed, order)).toEqual(["a:12-20"]);
});

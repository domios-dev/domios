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

  it("clear resets everything", () => {
    expect(run(toggle("a"), { type: "clear" })).toBe(EMPTY_SELECTION);
  });

  it("orderedSelection follows transcript order, not pick order", () => {
    expect(orderedSelection(run(toggle("d"), toggle("a"), toggle("c")), ORDER)).toEqual(["a", "c", "d"]);
  });
});

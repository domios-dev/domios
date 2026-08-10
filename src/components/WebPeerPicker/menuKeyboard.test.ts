// Run: npx tsx src/components/WebPeerPicker/menuKeyboard.test.ts

import { describe, expect, finish, it } from "../../test/standaloneTest";
import { menuKeyAction } from "./menuKeyboard";

describe("peer menu keyboard", () => {
  it("walks down the items in visual order and wraps", () => {
    expect(menuKeyAction("ArrowDown", 0, 3)).toEqual({ kind: "focus", index: 1 });
    expect(menuKeyAction("ArrowDown", 1, 3)).toEqual({ kind: "focus", index: 2 });
    expect(menuKeyAction("ArrowDown", 2, 3)).toEqual({ kind: "focus", index: 0 });
  });

  it("walks up the items in visual order and wraps", () => {
    expect(menuKeyAction("ArrowUp", 2, 3)).toEqual({ kind: "focus", index: 1 });
    expect(menuKeyAction("ArrowUp", 0, 3)).toEqual({ kind: "focus", index: 2 });
  });

  it("enters the menu at the first item when focus is outside it", () => {
    expect(menuKeyAction("ArrowDown", -1, 3)).toEqual({ kind: "focus", index: 0 });
    expect(menuKeyAction("ArrowUp", -1, 3)).toEqual({ kind: "focus", index: 2 });
  });

  it("jumps to the ends", () => {
    expect(menuKeyAction("Home", 2, 3)).toEqual({ kind: "focus", index: 0 });
    expect(menuKeyAction("End", 0, 3)).toEqual({ kind: "focus", index: 2 });
  });

  it("closes on Escape even with nothing to focus", () => {
    expect(menuKeyAction("Escape", 1, 3)).toEqual({ kind: "close" });
    expect(menuKeyAction("Escape", -1, 0)).toEqual({ kind: "close" });
  });

  it("leaves every other key to the browser", () => {
    for (const key of ["Enter", " ", "a", "Tab", "ArrowLeft"]) {
      expect(menuKeyAction(key, 0, 3)).toBeNull();
    }
  });

  it("does nothing with an empty menu", () => {
    expect(menuKeyAction("ArrowDown", -1, 0)).toBeNull();
    expect(menuKeyAction("Home", -1, 0)).toBeNull();
  });
});

finish();

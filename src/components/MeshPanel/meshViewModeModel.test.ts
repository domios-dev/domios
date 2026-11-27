import { describe, expect, it } from "vitest";
import {
  answerKeepCanvas,
  answerSwitchToList,
  answerViewToggle,
  MESH_COLLAPSED_GROUPS_STORAGE_KEY,
  MESH_LIST_DEFAULT_THRESHOLD,
  MESH_LIST_SUGGESTION_DISMISSED_KEY,
  MESH_SUGGESTION_RESET_THRESHOLD,
  MESH_VIEW_MODE_STORAGE_KEY,
  parseMeshViewMode,
  readCollapsedMeshGroups,
  readListSuggestionDismissed,
  readMeshViewMode,
  resolveInitialMeshViewMode,
  transitionMeshViewMode,
  writeCollapsedMeshGroups,
  writeListSuggestionDismissed,
  writeMeshViewMode,
} from "./meshViewModeModel";

function memoryStorage() {
  const data = new Map<string, string>();
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    raw: data,
  };
}

describe("meshViewModeModel", () => {
  it("opens on canvas at or below 8 machines and list above 8 when no choice exists, and honours narrow screens", () => {
    expect(MESH_LIST_DEFAULT_THRESHOLD).toBe(8);
    expect(MESH_SUGGESTION_RESET_THRESHOLD).toBe(6);
    expect(resolveInitialMeshViewMode(1, null, false)).toBe("canvas");
    expect(resolveInitialMeshViewMode(8, null, false)).toBe("canvas");
    expect(resolveInitialMeshViewMode(9, null, false)).toBe("list");
    expect(resolveInitialMeshViewMode(3, null, true)).toBe("list");
  });

  it("lets an explicit choice beat both the count threshold and narrow default on open", () => {
    expect(resolveInitialMeshViewMode(15, "canvas", false)).toBe("canvas");
    expect(resolveInitialMeshViewMode(3, "canvas", true)).toBe("canvas");
    expect(resolveInitialMeshViewMode(2, "list", false)).toBe("list");
  });

  it("suggests switching on 8->9 while on canvas without auto-switching", () => {
    const result = transitionMeshViewMode({
      previousCount: 8,
      count: 9,
      currentView: "canvas",
      explicitChoice: null,
      dismissed: false,
      narrow: false,
      opening: false,
      suggestionVisible: false,
    });
    expect(result).toEqual({ view: "canvas", dismissed: false, suggestionVisible: true });
  });

  it("does not suggest on 8->9 while already on list and never auto-switches on count drops", () => {
    expect(
      transitionMeshViewMode({
        previousCount: 8,
        count: 9,
        currentView: "list",
        explicitChoice: null,
        dismissed: false,
        narrow: false,
        opening: false,
        suggestionVisible: false,
      }),
    ).toEqual({ view: "list", dismissed: false, suggestionVisible: false });

    expect(
      transitionMeshViewMode({
        previousCount: 12,
        count: 4,
        currentView: "list",
        explicitChoice: null,
        dismissed: false,
        narrow: false,
        opening: false,
        suggestionVisible: false,
      }),
    ).toEqual({ view: "list", dismissed: false, suggestionVisible: false });
  });

  it("records Switch to list, Keep canvas, and toggle-as-answer cleanly", () => {
    expect(answerSwitchToList(false)).toEqual({
      view: "list",
      explicitChoice: "list",
      dismissed: false,
      suggestionVisible: false,
    });
    expect(answerKeepCanvas()).toEqual({
      view: "canvas",
      explicitChoice: "canvas",
      dismissed: true,
      suggestionVisible: false,
    });
    expect(answerViewToggle("canvas", true, false)).toEqual({
      view: "canvas",
      explicitChoice: "canvas",
      dismissed: true,
      suggestionVisible: false,
    });
    expect(answerViewToggle("list", true, false)).toEqual({
      view: "list",
      explicitChoice: "list",
      dismissed: false,
      suggestionVisible: false,
    });
  });

  it("asks once across flapping 8->9->8->9 and resets dismissed only once count drops to 6 or fewer", () => {
    const firstCross = transitionMeshViewMode({
      previousCount: 8,
      count: 9,
      currentView: "canvas",
      explicitChoice: null,
      dismissed: false,
      narrow: false,
      opening: false,
      suggestionVisible: false,
    });
    expect(firstCross.suggestionVisible).toBe(true);

    const kept = answerKeepCanvas();
    const dropToEight = transitionMeshViewMode({
      previousCount: 9,
      count: 8,
      currentView: kept.view,
      explicitChoice: kept.explicitChoice,
      dismissed: kept.dismissed,
      narrow: false,
      opening: false,
      suggestionVisible: kept.suggestionVisible,
    });
    expect(dropToEight).toEqual({ view: "canvas", dismissed: true, suggestionVisible: false });

    const flapToNine = transitionMeshViewMode({
      previousCount: 8,
      count: 9,
      currentView: dropToEight.view,
      explicitChoice: kept.explicitChoice,
      dismissed: dropToEight.dismissed,
      narrow: false,
      opening: false,
      suggestionVisible: dropToEight.suggestionVisible,
    });
    expect(flapToNine).toEqual({ view: "canvas", dismissed: true, suggestionVisible: false });

    const dropToSix = transitionMeshViewMode({
      previousCount: 9,
      count: 6,
      currentView: flapToNine.view,
      explicitChoice: kept.explicitChoice,
      dismissed: flapToNine.dismissed,
      narrow: false,
      opening: false,
      suggestionVisible: flapToNine.suggestionVisible,
    });
    expect(dropToSix).toEqual({ view: "canvas", dismissed: false, suggestionVisible: false });

    const growBackToNine = transitionMeshViewMode({
      previousCount: 8,
      count: 9,
      currentView: dropToSix.view,
      explicitChoice: kept.explicitChoice,
      dismissed: dropToSix.dismissed,
      narrow: false,
      opening: false,
      suggestionVisible: dropToSix.suggestionVisible,
    });
    expect(growBackToNine).toEqual({ view: "canvas", dismissed: false, suggestionVisible: true });
  });

  it("initializes view on open without triggering a crossing capsule", () => {
    expect(
      transitionMeshViewMode({
        previousCount: null,
        count: 15,
        currentView: "canvas",
        explicitChoice: null,
        dismissed: false,
        narrow: false,
        opening: true,
        suggestionVisible: false,
      }),
    ).toEqual({ view: "list", dismissed: false, suggestionVisible: false });

    expect(
      transitionMeshViewMode({
        previousCount: null,
        count: 15,
        currentView: "list",
        explicitChoice: "canvas",
        dismissed: false,
        narrow: false,
        opening: true,
        suggestionVisible: false,
      }),
    ).toEqual({ view: "canvas", dismissed: false, suggestionVisible: false });
  });

  it("persists bounded view mode, dismissal flag, and valid collapsed group overrides", () => {
    const storage = memoryStorage();
    expect(readMeshViewMode(storage)).toBeNull();
    expect(parseMeshViewMode("grid")).toBeNull();

    writeMeshViewMode("list", storage);
    expect(storage.raw.get(MESH_VIEW_MODE_STORAGE_KEY)).toBe("list");
    expect(readMeshViewMode(storage)).toBe("list");

    expect(readListSuggestionDismissed(storage)).toBe(false);
    writeListSuggestionDismissed(true, storage);
    expect(storage.raw.get(MESH_LIST_SUGGESTION_DISMISSED_KEY)).toBe("1");
    expect(readListSuggestionDismissed(storage)).toBe(true);

    writeCollapsedMeshGroups({ offline: false, needs_attention: true }, storage);
    expect(readCollapsedMeshGroups(storage)).toEqual({ needs_attention: true, offline: false });

    storage.setItem(MESH_COLLAPSED_GROUPS_STORAGE_KEY, JSON.stringify({ cloud: true, unknown: true, offline: "yes" }));
    expect(readCollapsedMeshGroups(storage)).toEqual({ cloud: true });

    storage.setItem(MESH_COLLAPSED_GROUPS_STORAGE_KEY, "not-json");
    expect(readCollapsedMeshGroups(storage)).toEqual({});
  });
});

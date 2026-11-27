import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { toggleCollapsedGroup, type MeshMachineGroupId } from "./meshMachineListModel";
import {
  answerKeepCanvas,
  answerSwitchToList,
  answerViewToggle,
  readCollapsedMeshGroups,
  readListSuggestionDismissed,
  readMeshViewMode,
  resolveInitialMeshViewMode,
  transitionMeshViewMode,
  writeCollapsedMeshGroups,
  writeListSuggestionDismissed,
  writeMeshViewMode,
  type MeshCollapsedGroupOverrides,
  type MeshInventoryViewMode,
} from "./meshViewModeModel";

export interface MeshViewModeState {
  viewMode: MeshInventoryViewMode;
  explicitChoice: MeshInventoryViewMode | null;
  dismissed: boolean;
  suggestionVisible: boolean;
  collapsedOverrides: MeshCollapsedGroupOverrides;
  selectViewMode: (mode: MeshInventoryViewMode) => void;
  acceptSuggestion: () => void;
  keepCanvas: () => void;
  toggleGroup: (groupId: MeshMachineGroupId, rowCount: number) => void;
}

export function useMeshViewMode(machineCount: number | null, narrow: boolean): MeshViewModeState {
  const [explicitChoice, setExplicitChoice] = useState<MeshInventoryViewMode | null>(() => readMeshViewMode());
  const [dismissed, setDismissed] = useState<boolean>(() => readListSuggestionDismissed());
  const [viewMode, setViewMode] = useState<MeshInventoryViewMode>(() =>
    machineCount === null
      ? (readMeshViewMode() ?? (narrow ? "list" : "canvas"))
      : resolveInitialMeshViewMode(machineCount, readMeshViewMode(), narrow),
  );
  const [suggestionVisible, setSuggestionVisible] = useState(false);
  const [collapsedOverrides, setCollapsedOverrides] = useState<MeshCollapsedGroupOverrides>(() =>
    readCollapsedMeshGroups(),
  );

  const previousCountRef = useRef<number | null>(machineCount);
  const viewModeRef = useRef(viewMode);
  viewModeRef.current = viewMode;
  const dismissedRef = useRef(dismissed);
  dismissedRef.current = dismissed;
  const suggestionVisibleRef = useRef(suggestionVisible);
  suggestionVisibleRef.current = suggestionVisible;

  useLayoutEffect(() => {
    if (machineCount === null) return;
    const previousCount = previousCountRef.current;
    const opening = previousCount === null;
    if (!opening && previousCount === machineCount) return;
    const next = transitionMeshViewMode({
      previousCount,
      count: machineCount,
      currentView: viewModeRef.current,
      explicitChoice,
      dismissed: dismissedRef.current,
      narrow,
      opening,
      suggestionVisible: suggestionVisibleRef.current,
    });
    previousCountRef.current = machineCount;
    if (next.view !== viewModeRef.current) {
      viewModeRef.current = next.view;
      setViewMode(next.view);
    }
    if (next.dismissed !== dismissedRef.current) {
      dismissedRef.current = next.dismissed;
      setDismissed(next.dismissed);
      writeListSuggestionDismissed(next.dismissed);
    }
    if (next.suggestionVisible !== suggestionVisibleRef.current) {
      suggestionVisibleRef.current = next.suggestionVisible;
      setSuggestionVisible(next.suggestionVisible);
    }
  }, [machineCount, explicitChoice, narrow]);

  const selectViewMode = useCallback((mode: MeshInventoryViewMode) => {
    const answer = answerViewToggle(mode, suggestionVisibleRef.current, dismissedRef.current);
    viewModeRef.current = answer.view;
    dismissedRef.current = answer.dismissed;
    suggestionVisibleRef.current = false;
    setViewMode(answer.view);
    setExplicitChoice(answer.explicitChoice);
    setDismissed(answer.dismissed);
    setSuggestionVisible(false);
    writeMeshViewMode(answer.explicitChoice);
    writeListSuggestionDismissed(answer.dismissed);
  }, []);

  const acceptSuggestion = useCallback(() => {
    const answer = answerSwitchToList(dismissedRef.current);
    viewModeRef.current = answer.view;
    suggestionVisibleRef.current = false;
    setViewMode(answer.view);
    setExplicitChoice(answer.explicitChoice);
    setSuggestionVisible(false);
    writeMeshViewMode(answer.explicitChoice);
  }, []);

  const keepCanvas = useCallback(() => {
    const answer = answerKeepCanvas();
    viewModeRef.current = answer.view;
    dismissedRef.current = answer.dismissed;
    suggestionVisibleRef.current = false;
    setViewMode(answer.view);
    setExplicitChoice(answer.explicitChoice);
    setDismissed(answer.dismissed);
    setSuggestionVisible(false);
    writeMeshViewMode(answer.explicitChoice);
    writeListSuggestionDismissed(answer.dismissed);
  }, []);

  const toggleGroup = useCallback((groupId: MeshMachineGroupId, rowCount: number) => {
    setCollapsedOverrides((current) => {
      const next = toggleCollapsedGroup(current, groupId, rowCount);
      writeCollapsedMeshGroups(next);
      return next;
    });
  }, []);

  return {
    viewMode,
    explicitChoice,
    dismissed,
    suggestionVisible,
    collapsedOverrides,
    selectViewMode,
    acceptSuggestion,
    keepCanvas,
    toggleGroup,
  };
}

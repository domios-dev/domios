import { useCallback, useState } from "react";
import {
  isMachineGroupExpanded,
  loadMachineGroupPrefs,
  persistMachineGroupPrefs,
  setMachineGroupExpanded,
} from "./workspaceMachineGroupsModel";

export function useWorkspaceMachineGroups() {
  const [expandedOverflow, setExpandedOverflow] = useState<ReadonlySet<string>>(() => new Set());
  const [groupPrefs, setGroupPrefs] = useState(loadMachineGroupPrefs);

  const isOverflowExpanded = useCallback(
    (groupKey: string) => expandedOverflow.has(groupKey),
    [expandedOverflow],
  );

  const setOverflowExpanded = useCallback((peerId: string | null, expanded: boolean) => {
    const key = JSON.stringify([peerId]);
    setExpandedOverflow((previous) => {
      if (previous.has(key) === expanded) return previous;
      const next = new Set(previous);
      if (expanded) next.add(key);
      else next.delete(key);
      return next;
    });
  }, []);

  const isGroupExpanded = useCallback(
    (peerId: string | null, totalMachines: number) => isMachineGroupExpanded(peerId, totalMachines, groupPrefs),
    [groupPrefs],
  );

  const toggleGroupExpanded = useCallback((peerId: string | null, totalMachines: number) => {
    setGroupPrefs((previous) => {
      const next = setMachineGroupExpanded(previous, peerId, !isMachineGroupExpanded(peerId, totalMachines, previous));
      persistMachineGroupPrefs(next);
      return next;
    });
  }, []);

  return { isOverflowExpanded, setOverflowExpanded, isGroupExpanded, toggleGroupExpanded };
}

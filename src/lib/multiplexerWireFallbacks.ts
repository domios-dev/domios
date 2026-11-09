import type { ClosedTab } from "../../gen/ClosedTab";

export type ClosedTabView = Omit<ClosedTab, "restorable_launch"> & {
  index: number;
  orphaned: boolean;
  group_id: string;
  group_kind: string;
};

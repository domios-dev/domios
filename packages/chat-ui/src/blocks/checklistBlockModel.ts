/** One step's one-click help: a nested block shown in place, or a deep link the host opens. */
export type ChecklistItemAction =
  | { kind: "block"; label: string; source: string }
  | { kind: "link"; label: string };

export interface ChecklistItemInfo {
  id: string;
  label: string;
  hint: string;
  /** True or false from real state; null while that state is unknown. */
  done: boolean | null;
  action: ChecklistItemAction;
}

export interface ChecklistInfo {
  id: string;
  title: string;
  items: ChecklistItemInfo[];
}

export type ChecklistBlockState =
  | { kind: "missing" }
  | { kind: "ready"; checklist: ChecklistInfo };

const CHECKLIST_ID = /^[a-z0-9][a-z0-9-]{0,63}$/;

export function parseChecklistId(argument: string): string | null {
  const id = argument.trim();
  return CHECKLIST_ID.test(id) ? id : null;
}

export interface ChecklistProgress {
  done: number;
  total: number;
  /** Steps whose state is still being read. */
  unknown: number;
}

export function checklistProgress(items: readonly ChecklistItemInfo[]): ChecklistProgress {
  return {
    done: items.filter((item) => item.done === true).length,
    total: items.length,
    unknown: items.filter((item) => item.done === null).length,
  };
}

export function checklistStatusLabel(progress: ChecklistProgress): string {
  if (progress.total > 0 && progress.done === progress.total) return "All done";
  return `${progress.done} of ${progress.total} done`;
}

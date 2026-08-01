import { useSyncExternalStore } from "react";

interface CronSelection {
  cronId: string;
  workspaceId: string;
}

let selection: CronSelection | null = null;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

export const cronSelectionStore = {
  open(next: CronSelection) {
    selection = next;
    emit();
  },
  close() {
    if (!selection) return;
    selection = null;
    emit();
  },
  get(): CronSelection | null {
    return selection;
  },
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
};

export function useCronSelection(): CronSelection | null {
  return useSyncExternalStore(cronSelectionStore.subscribe, cronSelectionStore.get, () => null);
}

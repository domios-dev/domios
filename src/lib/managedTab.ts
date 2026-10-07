import type { Tab } from "./types";

export function managedTabPresentation(managed: Tab["managed"]): { locked: boolean; label: string | null } {
  if (!managed) return { locked: false, label: null };
  return {
    locked: !managed.manager_closed,
    label: managed.manager_closed
      ? `Manager ${managed.manager_name} closed. You can send messages.`
      : `Managed by ${managed.manager_name}`,
  };
}

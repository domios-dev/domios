export function memoryDetailIdentity(item: { title: string; id: string } | null): { title: string; identifier: string | null } {
  return { title: item?.title.trim() || "Memory item", identifier: item?.id ?? null };
}

export function memoryListControlsVisible(detailOpen: boolean): boolean {
  return !detailOpen;
}

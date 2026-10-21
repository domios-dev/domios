const SECTION_LABELS = { managers: "MANAGERS", workers: "WORKERS", agents: "AGENTS", utilities: "UTILITIES", jobs: "JOBS", services: "SERVICES" } as const;
export type SidebarSectionKind = keyof typeof SECTION_LABELS;

export function sidebarSectionModel(kind: SidebarSectionKind, count: number, working = 0) {
  const total = Number.isFinite(count) ? Math.max(0, Math.floor(count)) : 0;
  const active = Number.isFinite(working) ? Math.min(total, Math.max(0, Math.floor(working))) : 0;
  const label = `${SECTION_LABELS[kind]} ${total}`;
  const activity = active > 0 ? `${active} working` : null;
  return { label, count: total, working: active, activity, accessibleName: activity ? `${label}, ${activity}` : label };
}

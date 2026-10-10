import { WorkspaceCardGrid } from "../../../../src/components/Sidebar/WorkspaceCardGrid";
import { SidebarDnd, WS_PREFIX } from "../../../../src/components/Sidebar/sortable";
import type { TabStatus, Workspace } from "../../../../src/lib/types";
import { defineSpecimen } from "../types";

const noop = () => undefined;
const NAMES = ["Atlas", "Amber", "CodeTerm", "Kite", "Binaura", "Rekap", "EcoIQ"];

function workspaces(count: number): Workspace[] {
  return NAMES.slice(0, count).map((name) => ({ id: `ws-${name.toLowerCase()}`, name, tabs: [], active_tab_id: null, cwd: `/Developer/${name}` }));
}

type GridProps = { count: number; selected?: string | null; working?: string[] };

function GridSpecimen({ count, selected = null, working = [] }: GridProps) {
  const list = workspaces(count);
  const statuses: Record<string, TabStatus | null> = Object.fromEntries(list.map((workspace) => [workspace.id, working.includes(workspace.id) ? "Working" : null]));
  return <div className="gallery-sidebar" style={{ padding: 8 }}>
    <SidebarDnd handlers={{ reorderTabs: noop, reorderDisplay: noop, reorderWorkspaces: noop }}>
      <WorkspaceCardGrid workspaces={list} selectedWorkspaceId={selected} statuses={statuses} sortIds={list.map((workspace) => `${WS_PREFIX}${workspace.id}`)} onSelect={noop} onCreate={noop} />
    </SidebarDnd>
  </div>;
}

export default defineSpecimen({
  id: "workspace-cube-grid",
  title: "Workspace card grid",
  category: "Sidebar",
  where: "src/components/Sidebar/WorkspaceCardGrid.tsx",
  notes: "The shipped two-row workspace grid with its create card; working workspaces carry the activity-dots badge.",
  Component: GridSpecimen,
  states: [
    { id: "mixed", label: "Selected + working", props: { count: 4, selected: "ws-atlas", working: ["ws-amber"] } },
    { id: "unselected", label: "Nothing selected", props: { count: 4 } },
    { id: "working", label: "Several working", props: { count: 3, selected: "ws-codeterm", working: ["ws-atlas", "ws-amber"] } },
    { id: "paged", label: "Overflowing into pages", props: { count: 7, selected: "ws-kite" } },
  ],
});

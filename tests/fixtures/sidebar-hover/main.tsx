import { useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import "../../../src/styles/globals.css";
import { SidebarSectionHeader } from "../../../src/components/Sidebar/SidebarSectionHeader";
import {
  SidebarDnd,
  SortableContext,
  SortableRow,
  type ReorderHandlers,
  type SortableGroupData,
  verticalListSortingStrategy,
} from "../../../src/components/Sidebar/sortable";
import "./fixture.css";

const GENERAL_ROWS = ["turing", "rubin", "sagan"];
const utilityRows = ["Build shell", "QA shell", "Logs shell"];

function TabRow({
  id,
  title,
  subtitle,
  layoutRow = true,
}: {
  id: string;
  title: string;
  subtitle?: string;
  layoutRow?: boolean;
}) {
  return (
    <div
      className="sidebar-tab sidebar-tab-dimmed"
      data-testid="sidebar-tab"
      data-row-id={id}
      data-sidebar-hover-row
      data-sidebar-layout-row={layoutRow ? "true" : undefined}
    >
      <button className="sidebar-tab-activation" type="button" aria-label={`Open ${title}`}>
        <span className="sidebar-tab-body">
          <span className="sidebar-tab-body__title truncate">{title}</span>
          {subtitle && <span className="sidebar-tab-body__sub truncate">{subtitle}</span>}
        </span>
      </button>
      <button className="sidebar-tab-kebab" type="button" aria-label={`Actions for ${title}`}>
        <span aria-hidden>···</span>
      </button>
    </div>
  );
}

function WorkspaceCards() {
  return (
    <section className="sidebar-workspace-card-scroll" aria-label="Workspaces">
      <div className="sidebar-workspace-card-grid">
        {["EcoIQ", "General", "Research", "Build"].map((name) => (
          <div className="sidebar-workspace-card" key={name} role="listitem">
            <button
              className="sidebar-workspace-card-select"
              type="button"
              aria-label={`Select workspace ${name}`}
              data-sidebar-hover-row
            >
              <span className="sidebar-workspace-card-initials">{name.slice(0, 2)}</span>
            </button>
          </div>
        ))}
        <button
          className="sidebar-workspace-card-create"
          type="button"
          aria-label="Create workspace"
          data-sidebar-hover-row
        >
          +
        </button>
      </div>
    </section>
  );
}

function Fixture() {
  const [generalRows, setGeneralRows] = useState(GENERAL_ROWS);
  const handlers = useMemo<ReorderHandlers>(() => ({
    reorderWorkspaces: () => undefined,
    reorderTabs: ({ workspaceId, orderedTabIds }) => {
      if (workspaceId === "general") setGeneralRows(orderedTabIds);
    },
    reorderDisplay: () => undefined,
  }), []);
  const generalItems = generalRows.map((id) => `tab:${id}`);

  return (
    <SidebarDnd handlers={handlers}>
      <main className="sidebar-hover-fixture" data-testid="sidebar-hover-fixture">
        <WorkspaceCards />
        <section className="fixture-team" aria-label="EcoIQ Team">
          <h1 className="fixture-team-title">EcoIQ · Team</h1>
          <div className="fixture-section">
            <SidebarSectionHeader kind="managers" count={1} expanded touchMode={false} data-sidebar-hover-row data-sidebar-layout-row="true" />
            <TabRow id="qa-helper" title="QA Helper" subtitle="Orchestrator" />
          </div>
          <div className="fixture-section sidebar-team-member">
            <SidebarSectionHeader kind="workers" count={1} expanded touchMode={false} data-sidebar-hover-row data-sidebar-layout-row="true" />
            <TabRow id="meticulous" title="Meticulous" subtitle="Worker" />
            <div className="sidebar-tab-tree-row">
              <TabRow id="meticulous-browser" title="Meticulous’s Browser" subtitle="Browser" />
            </div>
          </div>
          <div className="fixture-section">
            <SidebarSectionHeader kind="utilities" count={utilityRows.length} expanded touchMode={false} data-sidebar-hover-row data-sidebar-layout-row="true" />
            {utilityRows.map((title) => <TabRow key={title} id={title} title={title} subtitle="Utility shell" />)}
          </div>
          <div className="fixture-section">
            <h2 className="fixture-general-title" data-sidebar-layout-row="true">GENERAL</h2>
            <div className="fixture-general-rows">
              <SortableContext items={generalItems} strategy={verticalListSortingStrategy}>
                {generalRows.map((id) => {
                  const data: SortableGroupData = { group: "tab", groupKey: "general", pinnedIds: [] };
                  const title = id === "sagan" ? "Sagan" : id === "rubin" ? "Rubin" : "Turing";
                  return (
                    <SortableRow key={id} id={`tab:${id}`} data={data}>
                      <TabRow id={id} title={title} />
                    </SortableRow>
                  );
                })}
              </SortableContext>
            </div>
          </div>
          <button className="sidebar-new-session" type="button" data-sidebar-hover-row data-sidebar-layout-row="true">
            + New session
          </button>
        </section>
      </main>
    </SidebarDnd>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);

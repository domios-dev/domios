import { useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import "../../../src/styles/globals.css";
import "../../../src/components/Sidebar/sidebarPanel.css";
import "../../../src/components/Sidebar/tabRow.css";
import { TabItem } from "../../../src/components/Sidebar/TabItem";
import { SidebarDnd, SortableContext, SortableRow, keepPressFocusInPlace } from "../../../src/components/Sidebar/sortable";
import "./fixture.css";

const WORKSPACES = [
  { id: "codeterm", tabs: 6 },
  { id: "research", tabs: 2 },
  { id: "eight", tabs: 14 },
  { id: "plugins", tabs: 1 },
  { id: "billing", tabs: 9 },
];

interface FixtureState {
  workspace: number;
  tab: number;
  generalRows: number;
  bottom: number;
  generalTab: number | null;
}

function Row({ id, active = false, onPress }: { id: string; active?: boolean; onPress?: () => void }) {
  return (
    <SortableRow id={`tab:${id}`} data={{ group: "tab", groupKey: "fixture", pinnedIds: [] }}>
      <TabItem tab={{ id, pty_id: null, tab_type: "Terminal", title: id, cwd: null,
        description: "An existing session with a stable description" }} index={0} isActive={active}
        onClick={onPress ?? (() => undefined)} onClose={() => undefined} canClose workspaceId="fixture" />
    </SortableRow>
  );
}

function Fixture() {
  const [state, setState] = useState<FixtureState>({ workspace: 0, tab: 0, generalRows: 2, bottom: 120, generalTab: null });
  const set = (next: Partial<FixtureState>) => setState((current) => ({ ...current, ...next }));
  Object.assign(window, {
    sidebarFixture: { set: (next: Partial<FixtureState>) => flushSync(() => set(next)), workspaces: WORKSPACES },
  });
  const workspace = WORKSPACES[state.workspace];
  return (
    <SidebarDnd handlers={{ reorderWorkspaces: () => undefined, reorderTabs: () => undefined, reorderDisplay: () => undefined }}>
      <SortableContext items={[...Array.from({ length: state.generalRows }, (_, index) => `tab:general-${index}`),
      ...Array.from({ length: workspace.tabs }, (_, index) => `tab:${workspace.id}-${index}`)]}>
        <aside className="ct-sidebar-panel fixture-panel flex flex-col overflow-x-hidden overflow-y-hidden" data-collapsed="false" data-testid="sidebar">
          <div className="ct-sidebar-content flex-1 min-w-0 overflow-y-scroll overflow-x-hidden" data-testid="viewport" onMouseDown={keepPressFocusInPlace}>
            <section className="sidebar-general-section" data-testid="general-section">
              <div className="sidebar-structural-header sidebar-general-header w-full" data-sticky="general">
                <span className="sidebar-structural-label">GENERAL</span>
              </div>
              {Array.from({ length: state.generalRows }, (_, index) => (
                <Row key={index} id={`general-${index}`} active={state.generalTab === index}
                  onPress={() => set({ generalTab: index })} />
              ))}
            </section>
            <section className="sidebar-structural-section">
              <div className="sidebar-structural-header" data-sticky="workspaces">
                <span className="sidebar-structural-label">WORKSPACES</span>
              </div>
              <div className="sidebar-workspace-card-scroll" data-testid="workspace-card-scroll">
                <div className="sidebar-workspace-card-grid fixture-grid">
                  {WORKSPACES.map((entry, index) => (
                    <div key={entry.id} className="sidebar-workspace-card" data-selected={index === state.workspace || undefined} />
                  ))}
                </div>
              </div>
              {workspace && (
                <div key={workspace.id} data-testid="workspace-detail">
                  <div className="sidebar-workspace-detail-header" data-sticky="detail">{workspace.id}</div>
                  {Array.from({ length: workspace.tabs }, (_, index) => (
                    <Row key={index} id={`${workspace.id}-${index}`} active={state.generalTab === null && state.tab === index}
                      onPress={() => set({ tab: index, generalTab: null })} />
                  ))}
                </div>
              )}
            </section>
          </div>
          <div className="fixture-bottom" style={{ height: state.bottom }} />
        </aside>
      </SortableContext>
    </SidebarDnd>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);

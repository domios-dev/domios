import { useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import "../../../src/styles/globals.css";
import "../../../src/components/Sidebar/sidebarPanel.css";
import "../../../src/components/Sidebar/tabRow.css";
import { useSidebarScrollAnchor } from "../../../src/components/Sidebar/useSidebarScrollAnchor";
import "./fixture.css";

/** Workspaces with different tab counts, so a cross-workspace switch changes the detail height like the real-data sidebar. */
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
}

function Row({ id, active = false, onPress }: { id: string; active?: boolean; onPress?: () => void }) {
  return (
    <div className="sidebar-tab fixture-row" data-tab-id={id} data-active={active || undefined}>
      {active && <div className="active-tab-pill fixture-pill" />}
      <button type="button" className="fixture-row-button" aria-label={`Open tab ${id}`} aria-current={active ? "page" : undefined} onClick={onPress}>
        {id}
      </button>
    </div>
  );
}

function Fixture() {
  const [state, setState] = useState<FixtureState>({ workspace: 0, tab: 0, generalRows: 2, bottom: 120 });
  const anchor = useSidebarScrollAnchor(true);
  const set = (next: Partial<FixtureState>) => setState((current) => ({ ...current, ...next }));
  Object.assign(window, {
    sidebarFixture: { set: (next: Partial<FixtureState>) => flushSync(() => set(next)), workspaces: WORKSPACES },
  });
  const workspace = WORKSPACES[state.workspace];
  return (
    <aside className="ct-sidebar-panel fixture-panel flex flex-col overflow-x-hidden overflow-y-hidden" data-collapsed="false" data-testid="sidebar">
      <div ref={anchor.viewportRef} className="ct-sidebar-content flex-1 min-w-0 overflow-y-auto overflow-x-hidden" data-testid="viewport">
        <div ref={anchor.bodyRef}>
          <section className="sidebar-general-section" data-testid="general-section">
            <div className="sidebar-structural-header sidebar-general-header w-full" data-sticky="general">
              <span className="sidebar-structural-label">GENERAL</span>
            </div>
            {Array.from({ length: state.generalRows }, (_, index) => (
              <Row key={index} id={`general-${index}`} />
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
                  <Row key={index} id={`${workspace.id}-${index}`} active={state.tab === index}
                    onPress={() => set({ tab: index })} />
                ))}
              </div>
            )}
          </section>
        </div>
        <div ref={anchor.spaceRef} aria-hidden="true" data-testid="trailing-space" />
      </div>
      <div className="fixture-bottom" style={{ height: state.bottom }} />
    </aside>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);

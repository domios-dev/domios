import "./mocks/setup";
import { useEffect, useState, type CSSProperties } from "react";
import { createRoot } from "react-dom/client";
import { MemorySnapshotCanvas } from "../../src/components/MemoryPanel/MemorySnapshotCanvas";
import { configureDaemonTransport } from "../../src/lib/daemonBootstrap";
import { mockHttp } from "./mocks/backend";
import { MemoryNodeDetail } from "../../src/components/MemoryPanel/MemoryNodeDetail";
import { MemoryListTab } from "../../src/components/MemoryPanel/MemoryListTab";
import { MemoryFriendsContext } from "../../src/components/MemoryPanel/MemoryProvenance";
import type { MemNode } from "../../src/components/MemoryPanel/types";
import type { ApiConfig } from "../../src/lib/types";
import { DEFAULT_THEME, themeToCSS } from "../../src/lib/themeCss";
import "../../src/styles/globals.css";

configureDaemonTransport({ origin: window.location.origin, token: "fixture" });
const friend = { uid: "fixture-friend", friendship_id: "fixture-friendship", displayName: "Alex" };
const memory: MemNode = {
  id: "fixture_project_map", title: "Project X delivery notes", kind: "fact",
  summary: null, area: "project-x", importance: 3, tags: [], has_vector: true, has_body: true,
  body: "Project X uses the amber basin for its next delivery.\n\nConfirm the destination with the team before preparing the handoff. The reference is `amber-basin-732`.",
  created_at: "2026-10-29T14:02:00Z",
  author: { kind: "neighbor", account_uid: friend.uid, friendship_id: friend.friendship_id }, pinned: true,
};
const owner: MemNode = { ...memory, id: "fixture_owner", title: "Weekly review", body: "Review open decisions each Friday.", author: { kind: "owner" }, pinned: false };
const api = { host: "memory.gallery.test", port: 80, token: "fixture" } as ApiConfig;
mockHttp(window.location.origin, "/api/mem/graph/labels", () => [memory, owner].map(({ id, title }) => ({ id, title })));
mockHttp(window.location.origin, "/api/mem/graph/edges", () => []);
mockHttp(window.location.origin, "/api/mem/items/fixture_project_map", () => memory);
mockHttp(window.location.origin, "/api/mem/items/fixture_owner", () => owner);
const metadataNodes = new Map([[memory.id, memory], [owner.id, owner]]);
const index = { byId: new Map([memory, owner].map((node, i) => [node.id, { id: node.id, kind: node.kind, importance: 3, has_vector: true, pinned: i === 0, x: i * 100, y: i * 60, cluster_id: 0 }])), clusters: [], clusterEdges: [], layoutVersion: 1, seq: 1, computedAt: "" };
index.byId.set("extent-start", { id: "extent-start", kind: "note", importance: 2, has_vector: true, pinned: false, x: -100, y: -100, cluster_id: 0 });
index.byId.set("extent-end", { id: "extent-end", kind: "note", importance: 2, has_vector: true, pinned: false, x: 200, y: 200, cluster_id: 0 });
const lightTheme = { ...DEFAULT_THEME, name: "github-light", background: "#ffffff", foreground: "#1f2328", blue: "#0969da", bright_black: "#57606a", red: "#cf222e" };

function Fixture() {
  const [light, setLight] = useState(false);
  const [view, setView] = useState("neighbor");
  const node = view === "owner" ? owner : memory;
  const theme = light ? lightTheme : DEFAULT_THEME;
  useEffect(() => {
    for (const [name, value] of Object.entries(themeToCSS(theme))) document.documentElement.style.setProperty(name, value);
  }, [theme]);
  return <MemoryFriendsContext.Provider value={[friend]}>
    <main data-ct-theme={light ? "light" : "dark"} style={{ ...themeToCSS(theme), fontFamily: "Inter, system-ui, sans-serif", minHeight: "100vh", background: "var(--ct-bg)", color: "var(--ct-fg)", padding: 16 } as CSSProperties}>
      <nav className="flex items-center flex-wrap gap-3 mb-4" aria-label="Fixture controls">
        <button className="ct-btn ct-btn-ghost" onClick={() => setLight(!light)}>{light ? "Dark theme" : "Light theme"}</button>
        <button className="ct-btn ct-btn-ghost" onClick={() => setView("neighbor")}>Neighbor detail</button>
        <button className="ct-btn ct-btn-ghost" onClick={() => setView("owner")}>Owner detail</button>
        <button className="ct-btn ct-btn-ghost" onClick={() => setView("list")}>Memory list</button>
        <button className="ct-btn ct-btn-ghost" onClick={() => setView("graph")}>Memory graph</button>
        <span className="text-xs text-[var(--ct-muted-fg)]">Synthetic review fixture</span>
      </nav>
      <div className="relative border border-[var(--ct-border)] rounded-lg overflow-hidden" style={{ width: "min(1200px, 100%)", height: 680 }}>
        {view === "graph" ? <MemorySnapshotCanvas key={theme.name} api={api} index={index} metadataNodes={metadataNodes} pulses={new Map()} registerFit={() => {}} registerRecenter={() => {}} selectedIdOverride={memory.id} /> : view === "list" ? <MemoryListTab api={api} nodes={new Map([[memory.id, memory], [owner.id, owner]])} selectedId={null} onSelectItem={() => {}} onViewInGraph={() => setView("neighbor")} updateNode={() => {}} removeNode={() => {}} /> : <>
          <div className="p-6 text-xs text-[var(--ct-muted-fg)]">Memory · Inspect</div>
          <MemoryNodeDetail key={node.id} api={api} node={node} nodes={new Map()} edges={[]} onUpdate={() => {}} onDelete={() => {}} onSelectNode={() => {}} />
        </>}
      </div>
    </main>
  </MemoryFriendsContext.Provider>;
}

createRoot(document.getElementById("root")!).render(<Fixture />);

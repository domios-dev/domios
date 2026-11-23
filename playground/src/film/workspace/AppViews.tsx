import { lazy, Suspense, useCallback, useMemo, useState } from "react";
import { JobsView } from "../../../../src/components/Jobs/JobsView";
import { Button } from "../../../../src/components/shared/Button";
import { MemorySnapshotCanvas } from "../../../../src/components/MemoryPanel/MemorySnapshotCanvas";
import type { NodeIndex } from "../../../../src/components/MemoryPanel/snapshotLogic";
import { MemoryListTab } from "../../../../src/components/MemoryPanel/MemoryListTab";
import { MeshPanel } from "../../../../src/components/MeshPanel/MeshPanel";
import { API, appConfig } from "../../gallery/fixtures";
import { MEMORY_NODES } from "./memories";
import { JOBS_WORKSPACE } from "../fixtures/jobs";

/** The Jobs view the sidebar's Jobs button opens: the workspace's crons and Blueprints from the jobs fixtures. */
export function JobsPane() {
  const workspaces = useMemo(() => [JOBS_WORKSPACE], []);
  return (
    <div className="h-full w-full overflow-hidden" style={{ background: "var(--ct-bg)" }}>
      <JobsView workspaces={workspaces} teams={[]} initialFilter={{ workspaceId: JOBS_WORKSPACE.id, teamId: null }} />
    </div>
  );
}

// The daemon's layout spirals new nodes at sqrt(n) * COORD_RANGE * 0.025 (codeterm-mem graph_layout/placement.rs);
// since 1.22 the graph sizes nodes against those coordinates, so the sample spiral uses the same unit.
const LAYOUT_UNIT = 1000 * 0.025;
const MEMORIES = new Map(MEMORY_NODES.map((node) => [node.id, node]));
const GRAPH: NodeIndex = {
  byId: new Map([...MEMORIES.values()].map((node, index) => [node.id, {
    id: node.id, x: Math.sqrt(index) * LAYOUT_UNIT * Math.cos(index * 2.4), y: Math.sqrt(index) * LAYOUT_UNIT * Math.sin(index * 2.4), kind: node.kind,
    importance: node.importance ?? 4, cluster_id: 0, has_vector: true, pinned: false,
  }])),
  clusters: [], clusterEdges: [], layoutVersion: 1, seq: 1, computedAt: "",
};
const PULSES = new Map();
const MemoryVolume = lazy(() => import("./MemoryVolume"));
const noop = () => undefined;

/** The Memory view's list: facts, notes and skills; selecting works, anything that would change a memory says so. */
export function MemoryPane({ notInDemo, paused, reducedMotion }: {
  notInDemo: (what: string) => void; paused: boolean; reducedMotion: boolean;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  const [mode, setMode] = useState<"3D" | "Graph" | "List">("3D");
  const [hasVolume, setHasVolume] = useState(true);
  const fallback = useCallback(() => {
    setHasVolume(false);
    setMode("Graph");
  }, []);
  return (
    <div className="flex h-full w-full flex-col overflow-hidden" style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
      <div className="flex items-center justify-between border-b border-[var(--ct-border)] px-4 py-2">
        <strong>Memory</strong>
        <div className="flex gap-1" role="group" aria-label="Memory view">
          {(["3D", "Graph", "List"] as const).map((view) => <Button key={view} size="sm"
            disabled={view === "3D" && !hasVolume} aria-pressed={mode === view} onClick={() => setMode(view)}>{view}</Button>)}
        </div>
      </div>
      <div className="relative min-h-0 flex-1">{mode === "3D" ? <Suspense fallback={null}>
        <MemoryVolume paused={paused} reducedMotion={reducedMotion} onFailed={fallback} />
      </Suspense> : mode === "Graph" ? <MemorySnapshotCanvas api={API} index={GRAPH} pulses={PULSES}
        registerFit={noop} registerRecenter={noop} selectedIdOverride={selected} onSelectedIdChange={setSelected} /> : <MemoryListTab
        api={API}
        nodes={MEMORIES}
        selectedId={selected}
        onSelectItem={setSelected}
        onViewInGraph={() => setMode("Graph")}
        updateNode={() => notInDemo("Editing a memory")}
        removeNode={() => notInDemo("Deleting a memory")}
      />}</div>
    </div>
  );
}

const MESH_CONFIG = (() => {
  const config = appConfig();
  return { ...config, mesh: { ...config.mesh, enabled: true } };
})();

/** The Mesh view the sidebar's Mesh button opens, over the three sample machines. */
export function MeshPane({ notInDemo, onClose, connected, onConnect }: {
  notInDemo: (what: string) => void; onClose: () => void; connected: boolean; onConnect: () => void;
}) {
  return (
    <div className="flex h-full w-full flex-col overflow-hidden" style={{ background: "var(--ct-bg)" }}>
      <div className="relative min-h-0 flex-1">
        <MeshPanel key={String(connected)} api={API} config={MESH_CONFIG} onConfigUpdate={async () => notInDemo("Changing mesh settings")} onClose={onClose} />
      </div>
      <div className="flex justify-end border-t border-[var(--ct-border)] px-4 py-2">
        {connected ? <span role="status" className="text-sm text-[var(--ct-muted-fg)]">Sample machine connected</span> :
          <Button size="sm" onClick={onConnect}>Connect sample machine</Button>}
      </div>
    </div>
  );
}

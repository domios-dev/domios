import { useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, CalendarClock, Computer, Network, RefreshCw, Server } from "lucide-react";
import { BusySpinner } from "../shared/BusyState";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import type { Service } from "./types";
import {
  buildMeshGraphLayout,
  buildMeshInventoryGraphModel,
  type MeshGraphNode,
  type MeshGraphResourceNode,
} from "./meshInventoryGraphModel";
import type { InventoryPeerIssue, InventoryViewState } from "./meshInventoryModel";

interface Props {
  resources: Service[];
  thisMachineName: string;
  peerIssues?: InventoryPeerIssue[];
  loading?: boolean;
  error?: string | null;
  selectedResourceKey?: string | null;
  onSelectResource: (resource: Service, resourceKey: string) => void;
  onRefresh?: () => void;
  onEscape: () => void;
}

const GRAPH_WIDTH = 960;
const GRAPH_HEIGHT = 500;

export function MeshInventoryGraph({
  resources,
  thisMachineName,
  peerIssues = [],
  loading = false,
  error = null,
  selectedResourceKey = null,
  onSelectResource,
  onRefresh,
  onEscape,
}: Props) {
  const rootRef = useRef<HTMLElement>(null);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const snapshot = useMemo(() => ({ resources, peerIssues, loading, error }), [resources, peerIssues, loading, error]);
  const model = useMemo(() => buildMeshInventoryGraphModel(snapshot, thisMachineName), [snapshot, thisMachineName]);
  const layout = useMemo(() => buildMeshGraphLayout(model, GRAPH_WIDTH, GRAPH_HEIGHT), [model]);
  const selectedId = selectedResourceKey ? `resource:${selectedResourceKey}` : selectedNodeId;
  const selectedResource = model.resources.find((node) => node.id === selectedId);
  const announcement = graphAnnouncement(model.state, model.totalResources, selectedResource, loading, error);

  useEffect(() => {
    if (selectedResourceKey) setSelectedNodeId(`resource:${selectedResourceKey}`);
  }, [selectedResourceKey]);

  const selectNode = (node: MeshGraphNode) => {
    setSelectedNodeId(node.id);
    if (node.type !== "machine") onSelectResource(node.resource, node.resourceKey);
  };

  const moveNavigatorFocus = (event: React.KeyboardEvent<HTMLButtonElement>, direction: number) => {
    const buttons = Array.from(rootRef.current?.querySelectorAll<HTMLButtonElement>("[data-mesh-graph-nav-item]") ?? []);
    const index = buttons.indexOf(event.currentTarget);
    if (index < 0 || buttons.length === 0) return;
    event.preventDefault();
    buttons[(index + direction + buttons.length) % buttons.length]?.focus();
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.code === "Escape") {
      event.preventDefault();
      onEscape();
    }
  };

  return (
    <section
      ref={rootRef}
      className="absolute inset-0 flex min-h-0 flex-col bg-[var(--ct-bg)]"
      aria-labelledby="mesh-inventory-graph-heading"
      onKeyDown={handleKeyDown}
      data-testid="mesh-inventory-graph"
    >
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-3">
        <div className="mr-auto flex items-center gap-2">
          <h1 id="mesh-inventory-graph-heading" className="m-0 text-sm font-semibold text-[var(--ct-fg)]">Ownership graph</h1>
          <span className={UI_TEXT.metaMuted} data-testid="mesh-graph-count">{model.machines.length} machines · {model.totalResources} resources</span>
        </div>
        {onRefresh && (
          <button type="button" onClick={onRefresh} className={`${UI_BUTTON.ghost} ${UI_BUTTON.icon}`} aria-label="Refresh inventory graph" data-testid="mesh-graph-refresh">
            <BusySpinner icon={RefreshCw} size={14} spinning={loading} />
          </button>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-auto pb-24">
        <GraphStatus state={model.state} error={error} peerIssues={model.peerIssues} />
        <div className="grid min-w-[760px] grid-cols-[minmax(0,1fr)_240px] gap-3 p-4">
          <div
            className="relative min-h-[500px] overflow-hidden rounded-xl border border-[var(--ct-border)] bg-[radial-gradient(ellipse_at_center,var(--ct-titlebar-bg),var(--ct-bg))]"
            style={{ height: layout.height, minHeight: layout.height }}
            role="img"
            aria-label={`Ownership graph with ${model.machines.length} machines and ${model.totalResources} resources. Use the graph navigator for an accessible equivalent.`}
            data-testid="mesh-graph-canvas"
          >
            <svg className="absolute inset-0 h-full w-full" viewBox={`0 0 ${layout.width} ${layout.height}`} preserveAspectRatio="none" aria-hidden="true">
              <defs>
                <marker id="mesh-ownership-arrow" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto">
                  <path d="M0,0 L7,3.5 L0,7 z" fill="var(--ct-accent)" />
                </marker>
              </defs>
              {model.edges.map((edge) => {
                const source = layout.points.get(edge.source);
                const target = layout.points.get(edge.target);
                if (!source || !target) return null;
                return <line key={edge.id} x1={source.x + 72} y1={source.y} x2={target.x - 78} y2={target.y} stroke="var(--ct-accent)" strokeOpacity=".55" strokeWidth="2" markerEnd="url(#mesh-ownership-arrow)" />;
              })}
            </svg>
            {model.nodes.map((node) => {
              const point = layout.points.get(node.id);
              if (!point) return null;
              const resource = node.type !== "machine";
              const selected = selectedId === node.id;
              return (
                <button
                  key={node.id}
                  type="button"
                  tabIndex={-1}
                  onClick={() => selectNode(node)}
                  className={`absolute w-[156px] -translate-x-1/2 -translate-y-1/2 rounded-lg border px-3 py-2 text-left shadow-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--ct-accent)] ${node.type === "machine" ? "border-[var(--ct-blue)]/70 bg-[var(--ct-blue)]/10" : node.type === "schedule" ? "border-[var(--ct-yellow)]/70 bg-[var(--ct-yellow)]/10" : "border-[var(--ct-green)]/70 bg-[var(--ct-green)]/10"} ${selected ? "ring-2 ring-[var(--ct-accent)]" : ""}`}
                  style={{ left: `${(point.x / layout.width) * 100}%`, top: `${(point.y / layout.height) * 100}%` }}
                  aria-label={graphNodeLabel(node)}
                  data-testid="mesh-graph-visual-node"
                  data-node-id={node.id}
                >
                  <span className="flex items-center gap-1.5 text-[10px] font-semibold text-[var(--ct-fg)]">
                    {node.type === "machine" ? <Computer size={13} aria-hidden="true" /> : node.type === "schedule" ? <CalendarClock size={13} aria-hidden="true" /> : <Server size={13} aria-hidden="true" />}
                    <span className="truncate">{node.label}</span>
                  </span>
                  <span className="mt-1 block truncate text-[9px] text-[var(--ct-muted-fg)]">{nodeStatusLabel(node)}</span>
                  {node.type === "machine" && <span className="mt-1 block text-[9px] text-[var(--ct-muted-fg)]">{node.resourceCount} resource{node.resourceCount === 1 ? "" : "s"}</span>}
                  {resource && (node.status.unavailableReason || node.status.error) && <span className="mt-1 block truncate text-[9px] text-[var(--ct-yellow)]">{node.status.error ?? "Access unavailable"}</span>}
                </button>
              );
            })}
            {model.nodes.length === 0 && <div className="absolute inset-0 flex items-center justify-center text-sm text-[var(--ct-muted-fg)]">No ownership nodes yet.</div>}
          </div>

          <nav aria-label="Graph navigator" className="min-w-0 rounded-xl border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] p-3" data-testid="mesh-graph-navigator">
            <div className="mb-2 flex items-center gap-1.5 text-[11px] font-semibold text-[var(--ct-fg)]"><Network size={13} aria-hidden="true" /> Graph navigator</div>
            <p className="mb-3 text-[10px] leading-snug text-[var(--ct-muted-fg)]">Arrow keys move between machines and resources. Enter or Space opens a resource. Escape returns to the mode switcher.</p>
            <ol className="m-0 flex max-h-[390px] list-none flex-col gap-1 overflow-auto p-0">
              {model.nodes.map((node) => (
                <li key={node.id}>
                  <button
                    type="button"
                    data-mesh-graph-nav-item
                    data-node-id={node.id}
                    data-resource-key={node.type === "machine" ? undefined : node.resourceKey}
                    aria-current={selectedId === node.id ? "true" : undefined}
                    aria-label={graphNodeLabel(node)}
                    onClick={() => selectNode(node)}
                    onKeyDown={(event) => {
                      if (event.code === "ArrowDown" || event.code === "ArrowRight") moveNavigatorFocus(event, 1);
                      else if (event.code === "ArrowUp" || event.code === "ArrowLeft") moveNavigatorFocus(event, -1);
                      else if (event.code === "Enter" || event.code === "Space") {
                        event.preventDefault();
                        selectNode(node);
                      }
                    }}
                    className={`flex w-full items-start gap-2 rounded-md border px-2 py-2 text-left text-[10px] transition-colors hover:bg-[var(--ct-hover)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--ct-accent)] ${selectedId === node.id ? "border-[var(--ct-accent)] bg-[var(--ct-accent)]/10" : "border-transparent"}`}
                  >
                    <NodeIcon node={node} />
                    <span className="min-w-0 flex-1"><span className="block truncate font-medium text-[var(--ct-fg)]">{node.label}</span><span className="mt-0.5 block truncate text-[9px] text-[var(--ct-muted-fg)]">{nodeStatusLabel(node)}</span></span>
                  </button>
                </li>
              ))}
            </ol>
          </nav>
        </div>
      </div>
      <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="mesh-graph-announcement">{announcement}</div>
    </section>
  );
}

function NodeIcon({ node }: { node: MeshGraphNode }) {
  if (node.type === "machine") return <Computer size={13} className="mt-0.5 shrink-0 text-[var(--ct-blue)]" aria-hidden="true" />;
  if (node.type === "schedule") return <CalendarClock size={13} className="mt-0.5 shrink-0 text-[var(--ct-yellow)]" aria-hidden="true" />;
  return <Server size={13} className="mt-0.5 shrink-0 text-[var(--ct-green)]" aria-hidden="true" />;
}

function graphNodeLabel(node: MeshGraphNode): string {
  const kind = node.type === "machine" ? "Machine" : node.type === "schedule" ? "Schedule" : "Service";
  return `${kind} ${node.label}, ${nodeStatusLabel(node)}${node.type === "machine" ? `, ${node.resourceCount} resources` : `, resource key ${node.resourceKey}`}`;
}

function nodeStatusLabel(node: MeshGraphNode): string {
  const freshness = node.status.freshness;
  if (node.status.error) return `${freshness}, partial fetch failure`;
  if (node.status.unavailableReason) return `${freshness}, access denied`;
  return `${freshness}${node.status.state ? `, ${node.status.state}` : ""}`;
}

function graphAnnouncement(
  state: InventoryViewState,
  totalResources: number,
  selectedResource: MeshGraphResourceNode | undefined,
  loading: boolean,
  error: string | null | undefined,
): string {
  const selection = selectedResource ? ` Selected ${selectedResource.label}, ${selectedResource.resourceKey}.` : " No resource selected.";
  if (state === "loading") return "Loading graph inventory.";
  if (error && totalResources > 0) return `Graph refresh failed: ${error}. Showing available resources.${selection}`;
  return `${totalResources} resources across graph inventory${loading ? ", refreshing cached data" : ""}.${selection}`;
}

function GraphStatus({ state, error, peerIssues }: { state: InventoryViewState; error: string | null | undefined; peerIssues: InventoryPeerIssue[] }) {
  if (state === "loading") return <div className="border-b border-[var(--ct-border)] px-4 py-2 text-xs text-[var(--ct-muted-fg)]" role="status">Loading ownership graph…</div>;
  if (state === "error") return <div className="border-b border-[var(--ct-border)] px-4 py-2 text-xs text-[var(--ct-red)]" role="alert">Ownership graph unavailable{error ? `: ${error}` : "."}</div>;
  if (state === "empty") return <div className="border-b border-[var(--ct-border)] px-4 py-2 text-xs text-[var(--ct-muted-fg)]" data-testid="mesh-graph-empty">No resources registered. The ownership graph is empty.</div>;
  if (state === "partial-error") return <div className="flex items-center gap-1.5 border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-2 text-xs text-[var(--ct-yellow)]" role="status"><AlertTriangle size={13} aria-hidden="true" />Some peers failed; healthy and cached resources remain visible.{peerIssues.length ? ` ${peerIssues.length} peer issue${peerIssues.length === 1 ? "" : "s"}.` : ""}</div>;
  if (state === "offline") return <div className="border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-2 text-xs text-[var(--ct-yellow)]" role="status">Some ownership is offline; cached resources remain visible and operations are disabled.</div>;
  if (state === "stale" || state === "loading-cached") return <div className="border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-2 text-xs text-[var(--ct-yellow)]" role="status">{state === "loading-cached" ? "Refreshing graph… showing cached resources." : "Some ownership is stale; last-known resources remain visible."}</div>;
  return null;
}

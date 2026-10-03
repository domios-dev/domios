import { useId, useMemo } from "react";
import { Bot, Clock3, Maximize2, Minus, Plus, Radar } from "lucide-react";
import { automationState } from "./model";
import { layoutAutomationGraph } from "./layout";
import { automationKey, type AutomationGraph, type AutomationNode } from "./types";
import { useAutomationViewport, type AutomationCamera } from "./useAutomationViewport";
import { AUTOMATION_TOOL, stateTone } from "./styles";

const KIND_ICON = { sentinel: Radar, cron: Clock3, worker: Bot } as const;
const EDGE_COLORS = { active: "var(--ct-accent)", observed: "var(--ct-fg)", configured: "var(--ct-muted-fg)" } as const;

function NodeCard({ node, selected, related, dimmed }: { node: AutomationNode; selected: boolean; related: boolean; dimmed: boolean }) {
  const Icon = KIND_ICON[node.identity.kind];
  const state = automationState(node);
  const detail = (node.identity.kind === "cron" ? node.schedule : node.purpose) ?? node.identity.id;
  const results = node.last_result?.handlers.length ?? 0;
  const border = selected ? "border-[var(--ct-accent)] ring-2 ring-[color-mix(in_srgb,var(--ct-accent)_35%,transparent)]" : related ? "border-[color-mix(in_srgb,var(--ct-accent)_55%,var(--ct-border))]" : "border-[var(--ct-border)]";
  return <span className={`flex h-full flex-col gap-1 rounded-lg border bg-[var(--ct-titlebar-bg)] px-3 py-2.5 shadow-[var(--ct-shadow-sm)] transition-opacity ${border} ${node.availability === "available" ? "" : "border-dashed"} ${dimmed ? "opacity-55" : ""}`}>
    <span className="flex items-center gap-1.5 text-[0.6875rem] uppercase tracking-wider text-[var(--ct-muted)]">
      <Icon size={12} aria-hidden="true" />
      <span>{node.identity.kind}{node.hidden ? " · Hidden" : ""}</span>
      <span className="ml-auto flex items-center gap-1 normal-case tracking-normal text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state)}`} aria-hidden="true" />{state}</span>
    </span>
    <span className="truncate text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]">{node.name}</span>
    <span className={`truncate text-xs text-[var(--ct-muted-fg)] ${node.identity.kind === "cron" ? "font-mono" : ""}`}>{detail}</span>
    <span className="mt-auto truncate text-[0.6875rem] text-[var(--ct-muted)]">{node.last_result ? `Last ${node.last_result.bucket} · ${results} ${results === 1 ? "result" : "results"}` : "No accepted result"}</span>
  </span>;
}

export function AutomationCanvas({ graph, selected, camera, onSelect }: { graph: AutomationGraph; selected: string | null; camera: AutomationCamera; onSelect: (node: AutomationNode) => void }) {
  const markerId = useId();
  const layout = useMemo(() => layoutAutomationGraph(graph), [graph]);
  const viewport = useAutomationViewport(layout.bounds, camera);
  const connected = new Set(selected ? layout.edges.filter((route) => route.from === selected || route.to === selected).flatMap((route) => [route.from, route.to]) : []);
  return <div className="relative min-w-0">
    <div className="absolute right-2 top-2 z-10 flex gap-1.5" role="group" aria-label="Graph view controls">
      <button type="button" className={AUTOMATION_TOOL} aria-label="Zoom out" title="Zoom out" onClick={() => viewport.zoom(-0.2)}><Minus size={14} aria-hidden="true" /></button>
      <button type="button" className={AUTOMATION_TOOL} aria-label="Zoom in" title="Zoom in" onClick={() => viewport.zoom(0.2)}><Plus size={14} aria-hidden="true" /></button>
      <button type="button" className={AUTOMATION_TOOL} onClick={viewport.fit}><Maximize2 size={13} aria-hidden="true" />Fit graph</button>
    </div>
    <div
      ref={viewport.containerRef}
      tabIndex={0}
      role="region"
      aria-label="Automation graph"
      aria-describedby={`${markerId}-hint`}
      onMouseDown={viewport.handlers.onMouseDown}
      onKeyDown={viewport.onKeyDown}
      onScroll={(event) => { event.currentTarget.scrollTop = 0; event.currentTarget.scrollLeft = 0; }}
      {...viewport.touchHandlers}
      className="relative h-[min(30rem,62vh)] min-h-[16rem] min-w-0 touch-none overflow-hidden bg-[var(--ct-bg)] bg-[radial-gradient(color-mix(in_srgb,var(--ct-fg)_9%,transparent)_1px,transparent_1px)] [background-size:20px_20px] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
      style={{ cursor: viewport.isDragging ? "grabbing" : "grab" }}
    >
      <p id={`${markerId}-hint`} className="sr-only">Arrow keys pan, plus and minus zoom, zero fits the graph. Tab moves between participants.</p>
      <div ref={viewport.layerRef} className="absolute left-0 top-0" style={{ width: layout.bounds.width, height: layout.bounds.height, transformOrigin: "0 0" }}>
        <svg width={layout.bounds.width} height={layout.bounds.height} className="pointer-events-none absolute inset-0 overflow-visible" aria-hidden="true">
          <defs>{Object.entries(EDGE_COLORS).map(([tone, color]) => <marker key={tone} id={`${markerId}-${tone}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill={color} /></marker>)}</defs>
          {layout.edges.map((route) => {
            const observed = route.edge.evidence.kind === "observed";
            const active = selected !== null && (route.from === selected || route.to === selected);
            const faded = selected !== null && !active;
            const tone = active ? "active" : observed ? "observed" : "configured";
            return <g key={route.edge.id} data-edge-kind={route.edge.evidence.kind} opacity={faded ? 0.3 : 1}>
              <title>{route.label.title}</title>
              <path d={route.path} fill="none" stroke={EDGE_COLORS[tone]} strokeWidth={(observed ? 2 : 1.25) + (active ? 0.75 : 0)} strokeDasharray={observed ? undefined : "6 5"} markerEnd={`url(#${markerId}-${tone})`} />
              <rect x={route.label.x} y={route.label.y} width={route.label.width} height={route.label.height} rx={route.label.height / 2} fill="var(--ct-titlebar-bg)" stroke={active ? "var(--ct-accent)" : "var(--ct-border)"} />
              <text x={route.label.x + route.label.width / 2} y={route.label.y + route.label.height / 2} dominantBaseline="central" textAnchor="middle" fill={active ? "var(--ct-fg)" : "var(--ct-muted-fg)"} fontSize="11">{route.label.text}</text>
            </g>;
          })}
        </svg>
        {graph.nodes.map((node, index) => {
          const box = layout.nodes[index].box;
          const nodeKey = automationKey(node.identity);
          const isSelected = selected === nodeKey;
          return <button
            key={nodeKey}
            type="button"
            className="absolute rounded-lg p-0 text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
            style={{ left: box.x, top: box.y, width: box.width, height: box.height }}
            aria-pressed={isSelected}
            aria-label={`${node.identity.kind}: ${node.name} · ${automationState(node)}`}
            onFocus={() => viewport.reveal(box)}
            onClick={() => onSelect(node)}
          ><NodeCard node={node} selected={isSelected} related={connected.has(nodeKey)} dimmed={selected !== null && !isSelected && !connected.has(nodeKey)} /></button>;
        })}
      </div>
    </div>
  </div>;
}

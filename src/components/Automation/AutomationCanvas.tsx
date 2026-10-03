import { useId } from "react";
import { bezierPath, resolveEdgeEnds } from "../Canvas/edges";
import { formatLayerTransform } from "../Canvas/useCanvasViewport";
import { automationState, layoutAutomationGraph } from "./model";
import { automationKey, type AutomationGraph, type AutomationNode } from "./types";
import { useAutomationViewport } from "./useAutomationViewport";

import { AUTOMATION_BUTTON } from "./styles";

export function AutomationCanvas({ graph, selected, onSelect }: { graph: AutomationGraph; selected: string | null; onSelect: (node: AutomationNode) => void }) {
  const markerId = useId();
  const layout = layoutAutomationGraph(graph);
  const key = JSON.stringify(graph.nodes.map((node) => automationKey(node.identity)));
  const viewport = useAutomationViewport(layout.bounds, key);
  return <div ref={viewport.containerRef} onMouseDown={viewport.handlers.onMouseDown} className="relative h-[28rem] min-w-0 overflow-hidden bg-[var(--ct-bg)]" role="region" aria-label="Automation graph" style={{ cursor: viewport.isDragging ? "grabbing" : "grab" }}>
    <div className="absolute right-2 top-2 z-10 flex gap-2">
      <button type="button" className={AUTOMATION_BUTTON} aria-label="Zoom out" onClick={() => viewport.zoomBy(-0.2)}>−</button>
      <button type="button" className={AUTOMATION_BUTTON} aria-label="Zoom in" onClick={() => viewport.zoomBy(0.2)}>+</button>
      <button type="button" className={AUTOMATION_BUTTON} onClick={() => viewport.fitToView(layout.bounds)}>Fit graph</button>
    </div>
    <div ref={viewport.layerRef} className="absolute left-0 top-0" style={{ width: layout.bounds.width, height: layout.bounds.height, transform: formatLayerTransform(viewport.transform), transformOrigin: "0 0" }}>
      <svg width={layout.bounds.width} height={layout.bounds.height} className="absolute inset-0 pointer-events-none" aria-hidden="true">
        <defs><marker id={markerId} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--ct-accent)" /></marker></defs>
        {graph.edges.map((edge, index) => {
          const ends = resolveEdgeEnds({ id: edge.id, kind: "handoff-chain", from: automationKey(edge.source), to: automationKey(edge.target) }, layout.byId);
          if (!ends) return null;
          const observed = edge.evidence.kind === "observed";
          return <g key={edge.id} data-edge-kind={edge.evidence.kind}>
            <path d={bezierPath(ends.a, ends.b, 0.45)} fill="none" stroke="var(--ct-accent)" strokeWidth={observed ? 2 : 1} strokeDasharray={observed ? undefined : "6 4"} markerEnd={`url(#${markerId})`} />
            <text x={(ends.a.x + ends.b.x) / 2} y={(ends.a.y + ends.b.y) / 2 - 10 - (index % 3) * 16} textAnchor="middle" fill="var(--ct-fg)" fontSize="11" paintOrder="stroke" stroke="var(--ct-bg)" strokeWidth="4">{edge.bucket} · {edge.handler_type} · {edge.handler_id} · {edge.evidence.kind}</text>
          </g>;
        })}
      </svg>
      {graph.nodes.map((node, index) => {
        const box = layout.nodes[index].box;
        const nodeKey = automationKey(node.identity);
        return <button key={nodeKey} type="button" className={`${AUTOMATION_BUTTON} absolute overflow-hidden`} style={{ left: box.x, top: box.y, width: box.width, height: box.height }} aria-pressed={selected === nodeKey} aria-label={`${node.identity.kind}: ${node.name}`} onClick={() => onSelect(node)}>
          <span className="block text-[var(--ct-muted-fg)]">{node.identity.kind}{node.hidden ? " · Hidden" : ""}</span>
          <span className="block truncate font-semibold">{node.name}</span>
          <span className="block truncate">{(node.identity.kind === "cron" ? node.schedule : node.purpose) ?? node.identity.id}</span>
          <span className="block">{automationState(node)}</span>
        </button>;
      })}
    </div>
  </div>;
}

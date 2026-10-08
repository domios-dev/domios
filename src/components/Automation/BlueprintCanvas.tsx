import { useCallback, useId, useLayoutEffect, useRef, type ReactNode } from "react";
import { LocateFixed, Minus, Plus } from "lucide-react";
import type { ViewportTransform } from "../Canvas/useCanvasViewport";
import { BlueprintNodeCard } from "./BlueprintNodeCard";
import { NODE_TYPE_LABEL } from "./blueprintInspector";
import type { BlueprintLayout } from "./blueprintLayout";
import type { BlueprintNodeType } from "./blueprintModel";
import type { ReplayFrame } from "./blueprintRunModel";
import { nodeStatus } from "./BlueprintNodeCard";
import { AUTOMATION_TOOL } from "./styles";
import { useAutomationViewport, type AutomationCamera } from "./useAutomationViewport";

const LEGEND: readonly BlueprintNodeType[] = ["trigger", "source", "agent", "logic", "action"];

export interface CanvasReplay {
  frame: ReplayFrame;
  /** A live run lights its path without dimming the rest of the graph. */
  live: boolean;
}

export function BlueprintCanvas({ layout, selected, camera, onSelect, replay, panelOpen = false, toolbar }: {
  layout: BlueprintLayout; selected: string | null; camera: AutomationCamera; onSelect: (key: string) => void;
  replay: CanvasReplay | null; panelOpen?: boolean; toolbar?: ReactNode;
}) {
  const hintId = useId();
  const container = useRef<HTMLDivElement | null>(null);
  const paintGrid = useCallback((t: ViewportTransform) => {
    const style = container.current?.style;
    if (!style) return;
    style.setProperty("--bp-grid", `${16 * t.scale}px`);
    style.setProperty("--bp-grid-x", `${t.x}px`);
    style.setProperty("--bp-grid-y", `${t.y}px`);
  }, []);
  const viewport = useAutomationViewport(layout, camera, selected, panelOpen, paintGrid);
  const attach = useCallback((element: HTMLDivElement | null) => {
    container.current = element;
    viewport.containerRef.current = element;
  }, [viewport.containerRef]);
  useLayoutEffect(() => paintGrid(viewport.transform), [paintGrid, viewport.transform]);
  const frame = replay?.frame ?? null;
  const types = new Set(layout.nodes.map((entry) => entry.node.type));
  return <div className="bp-scope flex min-h-0 min-w-0 flex-1 flex-col">
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2">
      <div className="flex gap-1.5" role="group" aria-label="Graph view controls">
        <button type="button" className={AUTOMATION_TOOL} aria-label="Zoom out" title="Zoom out" onClick={() => viewport.zoom(-0.2)}><Minus size={14} aria-hidden="true" /></button>
        <button type="button" className={AUTOMATION_TOOL} aria-label="Zoom in" title="Zoom in" onClick={() => viewport.zoom(0.2)}><Plus size={14} aria-hidden="true" /></button>
        <button type="button" className={AUTOMATION_TOOL} title="Readable view of the selected node, or the first stage" onClick={viewport.fit}><LocateFixed size={13} aria-hidden="true" />Fit</button>
      </div>
      <ul aria-label="Node types" className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-[var(--ct-muted-fg)]">
        {LEGEND.filter((type) => types.has(type)).map((type) => <li key={type} className="flex items-center gap-1.5"><span className="h-2 w-2 rounded-sm" style={{ background: `var(--bp-${type}, var(--ct-muted))` }} aria-hidden="true" />{NODE_TYPE_LABEL[type]}</li>)}
      </ul>
      {toolbar}
    </div>
    <div
      ref={attach}
      tabIndex={0}
      role="region"
      aria-label="Automation graph"
      aria-describedby={hintId}
      data-replaying={frame && !replay?.live ? "true" : undefined}
      onMouseDown={viewport.handlers.onMouseDown}
      onKeyDown={viewport.onKeyDown}
      onScroll={(event) => { event.currentTarget.scrollTop = 0; event.currentTarget.scrollLeft = 0; }}
      {...viewport.touchHandlers}
      className="bp-canvas relative min-h-[12rem] min-w-0 flex-1 touch-none overflow-hidden focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
      style={{ cursor: viewport.isDragging ? "grabbing" : "grab" }}
    >
      <p id={hintId} className="sr-only">Arrow keys pan, plus and minus zoom, zero returns to a readable view. Tab moves between nodes.</p>
      <div ref={viewport.layerRef} className="absolute left-0 top-0" style={{ width: layout.bounds.width, height: layout.bounds.height, transformOrigin: "0 0", visibility: viewport.measured ? undefined : "hidden" }}>
        {layout.frames.map((entry) => <div key={entry.stageKey} className="bp-frame" style={{ left: entry.box.x, top: entry.box.y, width: entry.box.width, height: entry.box.height }}>
          <div className="bp-frame__title"><span className="shrink-0">{entry.title}</span>{entry.purpose && <span className="bp-frame__purpose">{entry.purpose}</span>}</div>
        </div>)}
        <svg width={layout.bounds.width} height={layout.bounds.height} className="pointer-events-none absolute inset-0 overflow-visible" aria-hidden="true">
          {layout.wires.map((route) => {
            const lit = frame?.wires.has(route.wire.id) ?? false;
            return <g key={route.wire.id} data-wire={route.wire.id}>
              <path className="bp-wire" data-kind={route.wire.kind} data-lit={lit || undefined} d={route.path} />
              {lit && <path className="bp-flow" d={route.path} />}
              {frame?.travelling.includes(route.wire.id) && <circle className="bp-pulse" r="4.5" style={{ offsetPath: `path('${route.path}')`, offsetDistance: `${frame.progress * 100}%` }} />}
            </g>;
          })}
        </svg>
        {layout.nodes.map((laid) => {
          const { node, box } = laid;
          const status = nodeStatus(node);
          return <button
            key={node.key}
            type="button"
            className="bp-node"
            data-type={node.type}
            data-node-key={node.key}
            data-lit={frame?.nodes.has(node.key) || undefined}
            data-missing={node.action?.handler?.availability && node.action.handler.availability !== "available" ? "true" : undefined}
            aria-pressed={selected === node.key}
            aria-label={`${NODE_TYPE_LABEL[node.type]} node: ${node.title} · ${node.subtitle}${status ? ` · ${status}` : ""}`}
            style={{ left: box.x, top: box.y, width: box.width, height: box.height }}
            onFocus={() => viewport.reveal(box)}
            onClick={() => onSelect(node.key)}
          ><BlueprintNodeCard laid={laid} /></button>;
        })}
      </div>
    </div>
  </div>;
}

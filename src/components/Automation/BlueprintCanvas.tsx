import { useCallback, useId, useLayoutEffect, useRef, type ReactNode } from "react";
import { LocateFixed, Minus, Plus } from "lucide-react";
import type { ViewportTransform } from "../Canvas/useCanvasViewport";
import { BlueprintNodeCard } from "./BlueprintNodeCard";
import { blueprintLegend, blueprintNodeKinds } from "./blueprintKinds";
import type { BlueprintLayout } from "./blueprintLayout";
import type { ReplayFrame } from "./blueprintRunModel";
import { nodeStatus } from "./BlueprintNodeCard";
import { AUTOMATION_TOOL } from "./styles";
import { useAutomationViewport, type AutomationCamera } from "./useAutomationViewport";
import { useBlueprintMotion } from "./useBlueprintMotion";
import type { BlueprintReplayOutcome } from "./blueprintReplayOutcome";
import { BlueprintLiquidSignal } from "./BlueprintLiquidSignal";


export interface CanvasReplay {
  frame: ReplayFrame;
  outcome?: BlueprintReplayOutcome | null;
  /** A live run lights its path without dimming the rest of the graph. */
  live: boolean;
}

export function BlueprintCanvas({ layout, selected, camera, onSelect, replay, panelOpen = false, toolbar }: {
  layout: BlueprintLayout; selected: string | null; camera: AutomationCamera; onSelect: (key: string) => void;
  replay: CanvasReplay | null; panelOpen?: boolean; toolbar?: ReactNode;
}) {
  const hintId = useId();
  const container = useRef<HTMLDivElement | null>(null);
  const motion = useBlueprintMotion(container);
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
  const outcome = frame?.done ? replay?.outcome : null;
  const types = new Set(layout.nodes.map((entry) => entry.node.type));
  return <div className="bp-scope flex min-h-0 min-w-0 flex-1 flex-col">
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2">
      <div className="flex gap-1.5" role="group" aria-label="Graph view controls">
        <button type="button" className={AUTOMATION_TOOL} aria-label="Zoom out" title="Zoom out" onClick={() => viewport.zoom(-0.2)}><Minus size={14} aria-hidden="true" /></button>
        <button type="button" className={AUTOMATION_TOOL} aria-label="Zoom in" title="Zoom in" onClick={() => viewport.zoom(0.2)}><Plus size={14} aria-hidden="true" /></button>
        <button type="button" className={AUTOMATION_TOOL} title="Readable view of the selected node, or the first stage" onClick={viewport.fit}><LocateFixed size={13} aria-hidden="true" />Fit</button>
      </div>
      <ul aria-label="Node types" className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-[var(--ct-muted-fg)]">
        {blueprintLegend.filter((type) => types.has(type)).map((type) => <li key={type} className="flex items-center gap-1.5"><span className="h-2 w-2 rounded-sm" style={{ background: `var(--bp-${type}, var(--ct-muted))` }} aria-hidden="true" />{blueprintNodeKinds[type].label}</li>)}
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
              {lit && outcome?.failed && route.wire.to.node === outcome.nodeKey && route.wire.kind === "exec" && <rect className="bp-stop-marker" x={route.to.x - 7} y={route.to.y - 3} width="6" height="6" rx="1.5" />}
              {motion && frame?.travelling.includes(route.wire.id) && <BlueprintLiquidSignal path={route.path} progress={frame.progress} to={route.to} />}
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
            data-replay-outcome={outcome?.nodeKey === node.key ? outcome.failed ? "failed" : "finished" : undefined}
            data-missing={node.action?.handler?.availability && node.action.handler.availability !== "available" ? "true" : undefined}
            aria-pressed={selected === node.key}
            aria-label={`${blueprintNodeKinds[node.type].label} node: ${node.title} · ${node.subtitle}${status ? ` · ${status}` : ""}${outcome?.nodeKey === node.key ? ` · ${outcome.label}` : ""}`}
            style={{ left: box.x, top: box.y, width: box.width, height: box.height }}
            onFocus={() => viewport.reveal(box)}
            onClick={() => onSelect(node.key)}
          >{motion && frame?.nodes.has(node.key) && <span className="bp-absorb" data-replay-motion="absorb" aria-hidden="true" />}<BlueprintNodeCard laid={laid} />{outcome?.nodeKey === node.key && <span className="bp-outcome-badge" title={outcome.reason}>{outcome.failed ? "Error" : outcome.noAction ? "No action" : "Finished"}</span>}</button>;
        })}
      </div>
    </div>
  </div>;
}

import { useCallback, useId, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { CanvasToolbar } from "@codeterm/chat-ui";
import { CircleAlert, Network, PauseCircle } from "lucide-react";
import type { ViewportTransform } from "@codeterm/canvas-camera";
import { BlueprintNodeCard, evidenceText, type PinWatch } from "./NodeCard";
import { CanvasNotices, NoteCard } from "./CanvasNotes";
import { nodeLabel, type BlueprintGraphModel } from "./graphModel";
import { ROW_HEIGHT, type BlueprintLayout, type LaidOutBlueprintNode } from "./layout";
import { pinValue, portName, wireValue } from "./pinValues";
import { ValuePopover, VALUE_POPOVER_CLASS, type ValueProbe } from "./ValuePopover";
import { useAutomationViewport, type AutomationCamera } from "./viewport";

/** Pointer focus must not pan the camera, or the release lands on a different node than the press. */
function keyboardFocus(element: HTMLElement): boolean {
  try { return element.matches(":focus-visible"); } catch { return true; }
}

/** A run fact pinned above the node it happened at, so it follows the graph on pan and zoom. */
export interface CanvasCallout { nodeKey: string; tone: "failed" | "held"; label: string; lead: string; reason: string; detail: string | null }

export function BlueprintCanvas({ layout, label, selected, camera, onSelect, onOpen, lit, litNodes: reached, historical = false, active, callouts = [], notices, crumbs, panelOpen = false, toolbar, values }: {
  layout: BlueprintLayout; label: string; selected: string | null; camera: AutomationCamera; onSelect: (key: string) => void; onOpen?: (key: string) => void;
  lit: ReadonlySet<string>; litNodes?: ReadonlySet<string>; historical?: boolean; active?: string | null; callouts?: readonly CanvasCallout[]; notices?: ReactNode; crumbs?: ReactNode; panelOpen?: boolean; toolbar?: ReactNode;
  /** The model of the selected run; with it, data pins and wires show what flowed through them. */
  values?: BlueprintGraphModel;
}) {
  const [probe, setProbe] = useState<ValueProbe | null>(null);
  const leave = (event: ReactPointerEvent) => {
    const next = event.relatedTarget;
    if (next instanceof Element && next.closest(`.${VALUE_POPOVER_CLASS}, .bp-pin-watch, .bp-wire-hit`)) return;
    setProbe(null);
  };
  const watchFor = (laid: LaidOutBlueprintNode): PinWatch | undefined => values && values.mode === "run" ? {
    onEnter: (pin, rowTop) => {
      const value = pinValue(values, laid.node, pin);
      if (!value) return;
      setProbe({ key: `${laid.node.key}#${pin.id}`, side: pin.side, port: portName(pin), type: pin.type ?? (pin.id.startsWith("bind:") ? undefined : pin.detail), value,
        x: pin.side === "in" ? laid.box.x : laid.box.x + laid.box.width, y: laid.box.y + rowTop + ROW_HEIGHT / 2 });
    },
    onLeave: leave,
  } : undefined;
  const hintId = useId();
  const container = useRef<HTMLDivElement | null>(null);
  const paintGrid = useCallback((t: ViewportTransform) => {
    const style = container.current?.style;
    if (!style) return;
    let pitch = 20 * t.scale;
    while (pitch < 18) pitch *= 2;
    style.setProperty("--bp-grid", `${pitch}px`);
    style.setProperty("--bp-dot", `${Math.round(Math.min(11, 4 + 7 * t.scale) * 10) / 10}%`);
    style.setProperty("--bp-grid-x", `${t.x}px`);
    style.setProperty("--bp-grid-y", `${t.y}px`);
  }, []);
  const viewport = useAutomationViewport(layout, camera, selected, panelOpen, paintGrid);
  const attach = useCallback((element: HTMLDivElement | null) => {
    container.current = element;
    viewport.containerRef.current = element;
  }, [viewport.containerRef]);
  useLayoutEffect(() => paintGrid(viewport.transform), [paintGrid, viewport.transform]);
  const openable = selected && onOpen ? layout.byKey.get(selected) : undefined;
  const litNodes = reached ?? new Set(layout.wires.filter((route) => lit.has(route.wire.id)).flatMap((route) => [route.wire.from.node, route.wire.to.node]));
  return <div className="bp-scope flex min-h-0 min-w-0 flex-1 flex-col">
    <div
      ref={attach}
      tabIndex={0}
      role="region"
      aria-label={label}
      aria-describedby={hintId}
      data-replaying={historical ? "true" : undefined}
      onMouseDown={viewport.handlers.onMouseDown}
      onKeyDown={viewport.onKeyDown}
      onScroll={(event) => { event.currentTarget.scrollTop = 0; event.currentTarget.scrollLeft = 0; }}
      {...viewport.touchHandlers}
      className="bp-canvas relative min-h-[14rem] min-w-0 flex-1 touch-none overflow-hidden focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
      style={{ cursor: viewport.isDragging ? "grabbing" : "grab" }}
    >
      <CanvasToolbar label="Blueprint graph controls" onFit={viewport.showOverview} onZoomOut={() => viewport.zoom(-0.2)} onZoomIn={() => viewport.zoom(0.2)} extras={toolbar} />
      {crumbs && <div className="bp-canvas-crumbs" onMouseDown={(event) => event.stopPropagation()}>{crumbs}</div>}
      <CanvasNotices>{notices}</CanvasNotices>
      <p id={hintId} className="sr-only">Arrow keys pan, plus and minus zoom, zero returns to a readable view. Tab moves between nodes.</p>
      <div ref={viewport.layerRef} className="absolute left-0 top-0" style={{ width: layout.bounds.width, height: layout.bounds.height, transformOrigin: "0 0", visibility: viewport.measured ? undefined : "hidden" }}>
        {layout.frames.map((entry) => <div key={entry.scope} className="bp-frame" style={{ left: entry.box.x, top: entry.box.y, width: entry.box.width, height: entry.box.height }}>
          <div className="bp-frame__title"><span className="shrink-0">{entry.title}</span>{entry.purpose && <span className="bp-frame__purpose">{entry.purpose}</span>}</div>
        </div>)}
        {layout.note && <NoteCard laid={layout.note} />}
        <svg width={layout.bounds.width} height={layout.bounds.height} className="pointer-events-none absolute inset-0 overflow-visible" aria-hidden="true">
          {layout.wires.map((route) => <path key={route.wire.id} data-wire={route.wire.id} className="bp-wire" data-kind={route.wire.kind} data-lit={lit.has(route.wire.id) || undefined} d={route.path} fill="none" />)}
          {values?.mode === "run" && layout.wires.filter((route) => route.wire.kind === "data").map((route) => <path key={`hit:${route.wire.id}`} className="bp-wire-hit" d={route.path} fill="none" stroke="transparent"
            onPointerEnter={() => {
              const value = wireValue(values, route.wire);
              const target = layout.byKey.get(route.wire.to.node)?.node.inputs.find((pin) => pin.id === route.wire.to.pin);
              if (value) setProbe({ key: `wire:${route.wire.id}`, side: "in", port: target ? portName(target) : "value", value, x: route.to.x, y: route.to.y });
            }}
            onPointerLeave={leave} />)}
        </svg>
        {callouts.map((callout) => {
          const anchor = layout.byKey.get(callout.nodeKey);
          if (!anchor) return null;
          const Icon = callout.tone === "failed" ? CircleAlert : PauseCircle;
          return <div key={`${callout.tone}:${callout.nodeKey}`} role="status" aria-label={callout.label} className="bp-callout" data-tone={callout.tone} title={callout.detail ? `${callout.reason}
${callout.detail}` : callout.reason}
            style={{ left: anchor.box.x, top: anchor.box.y - 30, maxWidth: Math.max(anchor.box.width + 120, 280) }}>
            <Icon size={13} aria-hidden="true" /><span className="bp-callout__lead">{callout.lead}</span><span className="bp-callout__reason">{callout.reason}</span>
            {callout.detail && <span className="sr-only">{callout.detail}</span>}
          </div>;
        })}
        {layout.nodes.map((laid) => {
          const { node, box } = laid;
          return <button
            key={node.key}
            type="button"
            className="bp-node"
            data-type={node.type}
            data-node-key={node.key}
            data-state={node.evidence?.state}
            data-subgraph={node.callee ? node.callee.openable ? "openable" : "pinned" : undefined}
            data-lit={litNodes.has(node.key) || undefined}
            data-active={active === node.key || undefined}
            aria-current={active === node.key ? "step" : undefined}
            aria-pressed={selected === node.key}
            aria-label={`Node ${nodeLabel(node)}`}
            aria-description={[node.trigger?.label, node.callee ? `Sub-graph ${node.callee.name}${node.callee.openable && onOpen ? ", double-click or Enter to open" : ""}` : null, node.evidence ? evidenceText(node.evidence) : null].filter(Boolean).join(". ") || undefined}
            style={{ left: box.x, top: box.y, width: box.width, height: box.height }}
            onFocus={(event) => { if (keyboardFocus(event.currentTarget)) viewport.reveal(box); }}
            aria-keyshortcuts={onOpen && node.type === "call" ? "Enter" : undefined}
            onClick={() => onSelect(node.key)}
            onDoubleClick={onOpen && node.type === "call" ? () => onOpen(node.key) : undefined}
            onKeyDown={onOpen && node.type === "call" ? (event) => { if (event.code !== "Enter" && event.code !== "NumpadEnter") return; event.preventDefault(); onOpen(node.key); } : undefined}
          ><BlueprintNodeCard laid={laid} watch={watchFor(laid)} /></button>;
        })}
        {probe && <ValuePopover key={probe.key} probe={probe} onLeave={leave} />}
        {openable?.node.callee?.openable && onOpen && <button type="button" className="bp-open-pill" style={{ left: openable.box.x + 12, top: openable.box.y + openable.box.height + 16 }}
          onClick={() => onOpen(openable.node.key)}><Network size={12} aria-hidden="true" />Double-click to open sub-graph</button>}
      </div>
    </div>
  </div>;
}

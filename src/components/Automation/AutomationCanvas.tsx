import { useId } from "react";
import { Bell, Bot, CircleSlash, Clock3, Ellipsis, LocateFixed, Maximize2, Minus, OctagonX, Plus, Radar, SquareTerminal, Zap } from "lucide-react";
import { automationState } from "./model";
import { commandOutcome } from "./CommandResultDetails";
import type { ActionLeaf } from "./projection";
import { LABEL_LINE_HEIGHT } from "./layout";
import { automationKindLabel, type AutomationGraph, type AutomationNode } from "./types";
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
  return <span className={`${CARD} ${border} ${node.availability === "available" ? "" : "border-dashed"} ${dimmed ? "opacity-55" : ""}`}>
    <span className="flex items-center gap-1.5 text-[0.6875rem] uppercase tracking-wider text-[var(--ct-muted-fg)]">
      <Icon size={12} aria-hidden="true" />
      <span>{automationKindLabel(node.identity.kind)}{node.hidden ? " · Hidden" : ""}</span>
      <span className="ml-auto flex items-center gap-1 normal-case tracking-normal text-[var(--ct-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state)}`} aria-hidden="true" />{state}</span>
    </span>
    <span className="truncate text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]">{node.name}</span>
    <span className={`truncate text-xs text-[var(--ct-muted-fg)] ${node.identity.kind === "cron" ? "font-mono" : ""}`}>{detail}</span>
    <span className="mt-auto truncate text-[0.6875rem] text-[var(--ct-muted-fg)]">{node.last_result ? `Last ${node.last_result.bucket} · ${results} ${results === 1 ? "result" : "results"}` : "No accepted result"}</span>
  </span>;
}

const ACTION_ICON: Record<string, typeof Zap> = { command: SquareTerminal, stop: OctagonX, notify: Bell };
const CARD = "flex h-full flex-col gap-1 rounded-lg border bg-[var(--ct-titlebar-bg)] px-3 py-2.5 shadow-[var(--ct-shadow-sm)] transition-opacity";

/** What a declared action leaf says about itself; the label is the handler's own safe label, never command text. */
export function actionSummary(leaf: ActionLeaf) {
  const { handler, result } = leaf;
  if (leaf.overflow) return { kind: "More actions", name: `+${leaf.overflow} declared`, detail: "Open the participant for every outcome", state: "Not drawn", footer: "Graph shows a bounded set" };
  if (!handler) return { kind: "Outcome", name: leaf.bucket, detail: "No action declared", state: "Terminal", footer: "Ends here" };
  const label = typeof handler.label === "string" && handler.label.trim() ? handler.label : handler.id;
  const outcome = commandOutcome(result);
  const footer = leaf.taken ? "In last accepted result" : result ? "Earlier result" : "Not run";
  return { kind: handler.type, name: label, detail: `${leaf.bucket}${handler.target ? ` · ${handler.target.id}` : ""} · ${handler.availability}`, state: result?.status ?? "No result", footer: outcome ? `${footer} · ${outcome}` : footer };
}

function ActionCard({ leaf, related, dimmed }: { leaf: ActionLeaf; related: boolean; dimmed: boolean }) {
  const Icon = leaf.overflow ? Ellipsis : leaf.handler ? ACTION_ICON[leaf.handler.type] ?? Zap : CircleSlash;
  const summary = actionSummary(leaf);
  const border = related ? "border-[color-mix(in_srgb,var(--ct-accent)_55%,var(--ct-border))]" : "border-[var(--ct-border)]";
  return <span className={`${CARD} ${border} ${leaf.taken ? "" : "border-dashed"} ${dimmed ? "opacity-55" : ""}`}>
    <span className="flex items-center gap-1.5 text-[0.6875rem] uppercase tracking-wider text-[var(--ct-muted-fg)]">
      <Icon size={12} aria-hidden="true" />
      <span className="truncate">{summary.kind}</span>
      <span className="ml-auto flex shrink-0 items-center gap-1 normal-case tracking-normal text-[var(--ct-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(summary.state)}`} aria-hidden="true" />{summary.state}</span>
    </span>
    <span className="truncate text-[length:var(--ct-font-size-md)] font-semibold text-[var(--ct-fg)]">{summary.name}</span>
    <span className="truncate text-xs text-[var(--ct-muted-fg)]">{summary.detail}</span>
    <span className="mt-auto truncate text-[0.6875rem] text-[var(--ct-muted-fg)]">{summary.footer}</span>
  </span>;
}

function Legend() {
  return <div className="flex flex-wrap items-center gap-x-5 gap-y-1 text-xs text-[var(--ct-muted-fg)]">
    <span className="flex items-center gap-2"><svg width="28" height="6" aria-hidden="true"><line x1="0" y1="3" x2="28" y2="3" stroke="var(--ct-muted-fg)" strokeWidth="1.5" strokeDasharray="6 5" /></svg>Dashed: configured route</span>
    <span className="flex items-center gap-2"><svg width="28" height="6" aria-hidden="true"><line x1="0" y1="3" x2="28" y2="3" stroke="var(--ct-fg)" strokeWidth="2" /></svg>Solid: recorded execution</span>
    <span className="flex items-center gap-2"><span className="h-3 w-5 rounded-sm border border-dashed border-[var(--ct-muted-fg)]" aria-hidden="true" />Dashed card: declared action not in the last result</span>
  </div>;
}

/** `fill` stretches the canvas over the remaining bounded height; otherwise it keeps its own bounded height for embedding. */
export function AutomationCanvas({ graph, selected, camera, fill = false, onSelect }: { graph: AutomationGraph; selected: string | null; camera: AutomationCamera; fill?: boolean; onSelect: (node: AutomationNode) => void }) {
  const markerId = useId();
  const viewport = useAutomationViewport(graph, camera, selected);
  const { layout } = viewport;
  const connected = new Set(selected ? layout.edges.filter((route) => route.from === selected || route.to === selected).flatMap((route) => [route.from, route.to]) : []);
  return <div className={`flex min-w-0 flex-col ${fill ? "min-h-0 flex-1" : ""}`}>
    <div className="flex flex-wrap items-center gap-x-5 gap-y-2 px-4 py-2">
      <div className="flex gap-1.5" role="group" aria-label="Graph view controls">
        <button type="button" className={AUTOMATION_TOOL} aria-label="Zoom out" title="Zoom out" onClick={() => viewport.zoom(-0.2)}><Minus size={14} aria-hidden="true" /></button>
        <button type="button" className={AUTOMATION_TOOL} aria-label="Zoom in" title="Zoom in" onClick={() => viewport.zoom(0.2)}><Plus size={14} aria-hidden="true" /></button>
        <button type="button" className={AUTOMATION_TOOL} title="Readable view of the selected participant, or the root" onClick={viewport.fit}><LocateFixed size={13} aria-hidden="true" />Fit graph</button>
        <button type="button" className={AUTOMATION_TOOL} title="Whole graph at any size" onClick={viewport.showOverview}><Maximize2 size={13} aria-hidden="true" />Overview</button>
      </div>
      <Legend />
    </div>
    <div
      ref={viewport.containerRef}
      tabIndex={0}
      role="region"
      aria-label="Automation graph"
      aria-describedby={`${markerId}-hint`}
      data-orientation={layout.orientation}
      onMouseDown={viewport.handlers.onMouseDown}
      onKeyDown={viewport.onKeyDown}
      onScroll={(event) => { event.currentTarget.scrollTop = 0; event.currentTarget.scrollLeft = 0; }}
      {...viewport.touchHandlers}
      className={`relative ${fill ? "min-h-[16rem] flex-1" : "h-[min(36rem,64vh)] min-h-[16rem]"} min-w-0 touch-none overflow-hidden bg-[var(--ct-bg)] bg-[radial-gradient(color-mix(in_srgb,var(--ct-fg)_9%,transparent)_1px,transparent_1px)] [background-size:20px_20px] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]`}
      style={{ cursor: viewport.isDragging ? "grabbing" : "grab" }}
    >
      <p id={`${markerId}-hint`} className="sr-only">Arrow keys pan, plus and minus zoom, zero returns to a readable view. Overview shows the whole graph. Tab moves between participants and actions.</p>
      <div ref={viewport.layerRef} className="absolute left-0 top-0" style={{ width: layout.bounds.width, height: layout.bounds.height, transformOrigin: "0 0", visibility: viewport.measured ? undefined : "hidden" }}>
        <svg width={layout.bounds.width} height={layout.bounds.height} className="pointer-events-none absolute inset-0 overflow-visible" aria-hidden="true">
          <defs>{Object.entries(EDGE_COLORS).map(([tone, color]) => <marker key={tone} id={`${markerId}-${tone}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill={color} /></marker>)}</defs>
          {layout.edges.map((route) => {
            const { edge, label } = route;
            const solid = edge.evidence === "observed" || (edge.evidence === "declared" && edge.taken);
            const active = selected !== null && (route.from === selected || route.to === selected);
            const faded = selected !== null && !active;
            const tone = active ? "active" : solid ? "observed" : "configured";
            const untaken = edge.evidence === "declared" && !edge.taken;
            return <g key={edge.id} data-edge-kind={edge.source ? edge.evidence : undefined} data-declared-edge={edge.source ? undefined : edge.taken ? "taken" : "untaken"} opacity={faded ? 0.3 : untaken ? 0.7 : 1}>
              <title>{label.title}</title>
              <path d={route.path} fill="none" stroke={EDGE_COLORS[tone]} strokeWidth={(solid ? 2 : 1.25) + (active ? 0.75 : 0)} strokeDasharray={solid ? undefined : "6 5"} markerEnd={`url(#${markerId}-${tone})`} />
              {label.lines.length > 0 && <>
                <rect x={label.x} y={label.y} width={label.width} height={label.height} rx={9} fill="var(--ct-titlebar-bg)" stroke={active ? "var(--ct-accent)" : "var(--ct-border)"} />
                <text x={label.x + label.width / 2} textAnchor="middle" fontSize="11">{label.lines.map((line, index) => <tspan key={index} x={label.x + label.width / 2} y={label.y + label.height / 2 + (index - (label.lines.length - 1) / 2) * LABEL_LINE_HEIGHT} dominantBaseline="central" fill={index === 0 || active ? "var(--ct-fg)" : "var(--ct-muted-fg)"}>{line}</tspan>)}</text>
              </>}
            </g>;
          })}
        </svg>
        {layout.items.map((item, index) => {
          const box = layout.nodes[index].box;
          const style = { left: box.x, top: box.y, width: box.width, height: box.height };
          const frame = "absolute rounded-lg p-0 text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]";
          const related = connected.has(item.key);
          const dimmed = selected !== null && selected !== item.key && !related;
          if (item.action) {
            const { action } = item;
            const summary = actionSummary(action);
            return <button key={item.key} type="button" className={frame} style={style} data-action-key={item.key}
              aria-label={`${summary.kind} from ${action.source.name}: ${summary.name} · ${summary.state} · ${summary.footer}`}
              onFocus={() => viewport.reveal(box)} onClick={() => onSelect(action.source)}
            ><ActionCard leaf={action} related={related} dimmed={dimmed} /></button>;
          }
          const node = item.participant;
          const isSelected = selected === item.key;
          return <button
            key={item.key}
            type="button"
            className={frame}
            style={style}
            aria-pressed={isSelected}
            aria-label={`${automationKindLabel(node.identity.kind)}: ${node.name} · ${automationState(node)}`}
            onFocus={() => viewport.reveal(box)}
            onClick={() => onSelect(node)}
          ><NodeCard node={node} selected={isSelected} related={related} dimmed={dimmed} /></button>;
        })}
      </div>
    </div>
  </div>;
}

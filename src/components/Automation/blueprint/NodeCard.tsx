import type { PointerEvent as ReactPointerEvent } from "react";
import { HEADER_HEIGHT, STATUS_HEIGHT, type LaidOutBlueprintNode } from "./layout";
import type { BlueprintPin, NodeEvidence } from "./graphModel";
import { stateTone } from "../styles";

function Pin({ pin }: { pin: BlueprintPin }) {
  return <span className="bp-pin" data-side={pin.side} data-kind={pin.kind} data-connected={pin.connected || undefined} aria-hidden="true" />;
}

/** A bound input names where its value comes from; the source's default `out` port goes unsaid. */
function shownDetail(pin: BlueprintPin): string | undefined {
  if (!pin.detail || !pin.id.startsWith("bind:")) return pin.detail;
  const split = pin.detail.indexOf(".");
  const source = split > 0 && pin.detail.slice(split + 1) === "out" ? pin.detail.slice(0, split) : pin.detail;
  return `← ${source}`;
}

function PinLabel({ pin }: { pin: BlueprintPin }) {
  if (!pin.label) return null;
  const detail = pin.kind === "data" ? shownDetail(pin) : undefined;
  return <span className="bp-pin-label" data-side={pin.side} data-kind={pin.kind} title={pin.detail ? `${pin.label}: ${pin.detail}` : pin.label}>
    <span className="bp-pin-label__name">{pin.label}</span>
    {detail && <span className="bp-pin-label__detail">{detail}</span>}
  </span>;
}

export function evidenceText(evidence: NodeEvidence): string {
  return [evidence.outcome && evidence.outcome !== evidence.state ? `${evidence.state} → ${evidence.outcome}` : evidence.state, evidence.summary].filter(Boolean).join(" · ");
}

export interface PinWatch {
  onEnter: (pin: BlueprintPin, rowTop: number) => void;
  onLeave: (event: ReactPointerEvent) => void;
}

function PinSlot({ pin, rowTop, watch }: { pin: BlueprintPin; rowTop: number; watch?: PinWatch }) {
  const parts = pin.side === "in" ? <><Pin pin={pin} /><PinLabel pin={pin} /></> : <><PinLabel pin={pin} /><Pin pin={pin} /></>;
  if (!watch || pin.kind !== "data") return parts;
  return <span className="bp-pin-watch" data-side={pin.side} onPointerEnter={() => watch.onEnter(pin, rowTop)} onPointerLeave={watch.onLeave}>{parts}</span>;
}

/** Visual body of one typed node; geometry comes from the layout so pins land where wires end. */
export function BlueprintNodeCard({ laid, watch }: { laid: LaidOutBlueprintNode; watch?: PinWatch }) {
  const { node, bodyHeight, bodyLines, statusHeight, rows } = laid;
  const Icon = node.icon;
  return <>
    <span className="bp-node__header" style={{ height: HEADER_HEIGHT }}>
      <span className="bp-node__icon" aria-hidden="true"><Icon size={14} /></span>
      <span className="bp-node__titles">
        <span className="bp-node__title">{node.title}</span>
        <span className="bp-node__subtitle">{node.callee ? `${node.kindLabel} · ${node.callee.name}` : node.kindLabel}{node.callPath.length ? ` · in ${node.callPath.join(" / ")}` : ""}</span>
      </span>
    </span>
    {node.evidence && <span className="bp-node__status" data-state={node.evidence.state} style={{ height: statusHeight || STATUS_HEIGHT }}>
      <span className={`bp-node__dot ${stateTone(node.evidence.state)}`} aria-hidden="true" />
      <span className="bp-node__status-text">{evidenceText(node.evidence)}</span>
    </span>}
    {node.body && <span className="bp-node__body" data-kind={node.body.kind} style={{ height: bodyHeight }}>
      <span className="bp-node__clamp" style={{ WebkitLineClamp: bodyLines }}>{node.body.text}</span>
    </span>}
    {node.callee?.openable && <span className="bp-node__hint" aria-hidden="true">Double-click to open</span>}
    {rows.map((row, index) => <span key={index} className="bp-row" data-pair={row.left?.label && row.right?.label ? "both" : "single"} style={{ top: row.top, height: row.height }}>
      {row.left && <PinSlot pin={row.left} rowTop={row.top} watch={watch} />}
      {row.right && <PinSlot pin={row.right} rowTop={row.top} watch={watch} />}
    </span>)}
  </>;
}

import { HEADER_HEIGHT, STATUS_HEIGHT, type LaidOutBlueprintNode } from "./layout";
import type { BlueprintPin, NodeEvidence } from "./graphModel";
import { stateTone } from "../styles";

function Pin({ pin }: { pin: BlueprintPin }) {
  return <span className="bp-pin" data-side={pin.side} data-kind={pin.kind} data-connected={pin.connected || undefined} aria-hidden="true" />;
}

function PinLabel({ pin }: { pin: BlueprintPin }) {
  if (!pin.label) return null;
  return <span className="bp-pin-label" data-side={pin.side} data-kind={pin.kind} title={pin.detail ? `${pin.label}: ${pin.detail}` : undefined}>
    <span className="bp-pin-label__name">{pin.label}</span>
    {pin.kind === "data" && pin.detail && <span className="bp-pin-label__detail">{pin.detail}</span>}
  </span>;
}

export function evidenceText(evidence: NodeEvidence): string {
  return [evidence.outcome && evidence.outcome !== evidence.state ? `${evidence.state} → ${evidence.outcome}` : evidence.state, evidence.summary].filter(Boolean).join(" · ");
}

/** Visual body of one typed node; geometry comes from the layout so pins land where wires end. */
export function BlueprintNodeCard({ laid }: { laid: LaidOutBlueprintNode }) {
  const { node, bodyHeight, bodyLines, statusHeight, rows } = laid;
  const Icon = node.icon;
  return <>
    <span className="bp-node__header" style={{ height: HEADER_HEIGHT }}>
      <span className="bp-node__icon" aria-hidden="true"><Icon size={14} /></span>
      <span className="bp-node__titles">
        <span className="bp-node__title">{node.title}</span>
        <span className="bp-node__subtitle">{node.kindLabel}{node.callPath.length ? ` · in ${node.callPath.join(" / ")}` : ""}</span>
      </span>
    </span>
    {node.evidence && <span className="bp-node__status" data-state={node.evidence.state} style={{ height: statusHeight || STATUS_HEIGHT }}>
      <span className={`bp-node__dot ${stateTone(node.evidence.state)}`} aria-hidden="true" />
      <span className="bp-node__status-text">{evidenceText(node.evidence)}</span>
    </span>}
    {node.body && <span className="bp-node__body" data-kind={node.body.kind} style={{ height: bodyHeight }}>
      <span className="bp-node__clamp" style={{ WebkitLineClamp: bodyLines }}>{node.body.text}</span>
    </span>}
    {rows.map((row, index) => <span key={index} className="bp-row" style={{ top: row.top, height: row.height }}>
      {row.left && <><Pin pin={row.left} /><PinLabel pin={row.left} /></>}
      {row.right && <><PinLabel pin={row.right} /><Pin pin={row.right} /></>}
    </span>)}
  </>;
}

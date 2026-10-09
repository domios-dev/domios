import { nodeIcon } from "./blueprintKinds";
export { nodeIcon } from "./blueprintKinds";
import { HEADER_HEIGHT, type LaidOutBlueprintNode } from "./blueprintLayout";
import type { BlueprintNode, BlueprintPin } from "./blueprintModel";
import { automationState } from "./model";
import { stateTone } from "./styles";

function Pin({ pin }: { pin: BlueprintPin }) {
  const stroke = pin.kind === "exec" ? "var(--bp-exec)" : "var(--bp-data)";
  const fill = pin.connected ? stroke : "var(--ct-titlebar-bg)";
  return <svg className="bp-pin" data-side={pin.side} data-kind={pin.kind} viewBox="0 0 12 12" aria-hidden="true">
    {pin.kind === "exec"
      ? <path d="M1.5 1.5 H6.5 L10.5 6 L6.5 10.5 H1.5 Z" fill={fill} stroke={stroke} strokeWidth="1.4" strokeLinejoin="round" />
      : <circle cx="6" cy="6" r="4.2" fill={fill} stroke={stroke} strokeWidth="1.6" />}
  </svg>;
}

function PinLabel({ pin }: { pin: BlueprintPin }) {
  if (!pin.label) return null;
  return <span className="bp-pin-label" data-side={pin.side} title={pin.detail ? `${pin.label}: ${pin.detail}` : undefined}>
    <span className={pin.kind === "exec" ? "bp-pin-label__name" : undefined}>{pin.label}</span>
  </span>;
}

export function nodeStatus(node: BlueprintNode): string | null {
  if (node.type === "trigger") return automationState(node.stage);
  if (node.type === "action") return node.action?.result?.status ?? null;
  return null;
}

/** Visual body of one typed node; geometry comes from the layout so pins land where wires end. */
export function BlueprintNodeCard({ laid }: { laid: LaidOutBlueprintNode }) {
  const { node, bodyHeight, bodyLines, rows } = laid;
  const Icon = nodeIcon(node);
  const status = nodeStatus(node);
  return <>
    <span className="bp-node__header" style={{ height: HEADER_HEIGHT }}>
      <Icon size={15} className="bp-node__icon" aria-hidden="true" />
      <span className="bp-node__titles">
        <span className="bp-node__title">{node.title}</span>
        <span className="bp-node__subtitle">{node.subtitle}</span>
      </span>
      {status && <span className={`bp-node__status ${stateTone(status)}`} title={status} aria-hidden="true" />}
    </span>
    {node.body && <span className="bp-node__body" data-kind={node.body.kind} style={{ height: bodyHeight }}>
      {node.body.kind === "prompt" && <span className="bp-node__caption">Instruction · what each output means</span>}
      <span className="bp-node__clamp" style={{ WebkitLineClamp: bodyLines }}>{node.body.text}</span>
    </span>}
    {rows.map((row, index) => <span key={index} className="bp-row" style={{ top: row.top, height: row.height }}>
      {row.left && <><Pin pin={row.left} /><PinLabel pin={row.left} /></>}
      {row.right && <><PinLabel pin={row.right} /><Pin pin={row.right} /></>}
    </span>)}
  </>;
}

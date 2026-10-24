import { Fragment, type CSSProperties, type RefObject } from "react";
import { CodeBlock } from "@codeterm/chat-ui";
import type { BlueprintExpandedCall } from "../../../lib/blueprintApi";
import type { PresenceState } from "../../Canvas/usePresence";
import { BlueprintProjectionValue } from "../BlueprintProjectionValue";
import { DetailsFrame, Disclosure, Facts } from "../DetailsFrame";
import { Button } from "../../shared/Button";
import { stateTone } from "../styles";
import { nodeLabel, portType, type BlueprintGraphModel, type BlueprintGraphNode } from "./graphModel";
import { CodeAside } from "./CodeAside";

const SECTION = "ct-detail__heading";

function eventTime(value: number | null): string | null {
  return value === null ? null : new Date(value).toLocaleString();
}

export function BlueprintNodeDetails({ node, model, call, expanded, onExpand, onOpen, onClose, panelRef, presence }: {
  node: BlueprintGraphNode | null; model: BlueprintGraphModel; call?: BlueprintExpandedCall; expanded: boolean;
  onExpand: () => void; onOpen?: () => void; onClose: () => void; panelRef?: RefObject<HTMLElement | null>; presence?: PresenceState;
}) {
  if (!node) return <DetailsFrame label="Details: unavailable node" title={<h3 className="plan-side-panel__title">Node unavailable</h3>} onClose={onClose} panelRef={panelRef} presence={presence} focusWhenLost>
    <p role="status" className="ct-detail__empty">The selected node is no longer part of this Blueprint.</p>
  </DetailsFrame>;
  const Icon = node.icon;
  const titles = new Map(model.nodes.filter((entry) => entry.scope === node.scope).map((entry) => [entry.id, entry.title]));
  const definition = node.definition;
  const time = node.evidence ? eventTime(node.evidence.at) : null;
  return <DetailsFrame label={`Details: ${nodeLabel(node)}`} onClose={onClose} panelRef={panelRef} presence={presence} title={<div className="bp-scope flex min-w-0 items-center gap-3">
    <span className="bp-detail-icon" data-type={node.type} style={{ "--bp-type": `var(--bp-${node.type})` } as CSSProperties}><Icon size={15} aria-hidden="true" /></span>
    <span className="min-w-0">
      <h3 className="plan-side-panel__title">{node.title}</h3>
      <span className="text-xs text-[var(--ct-muted-fg)]">{node.kindLabel} node{node.callPath.length ? ` · in ${node.callPath.join(" / ")}` : ""}</span>
    </span>
  </div>}>
    {node.evidence
      ? <section aria-label="Run evidence" className="bp-evidence" data-state={node.evidence.state}>
        <span className={`bp-node__dot ${stateTone(node.evidence.state)}`} aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <p className="m-0 font-semibold">{node.evidence.state}{node.evidence.outcome && node.evidence.outcome !== node.evidence.state ? ` → ${node.evidence.outcome}` : ""}</p>
          {node.evidence.summary && <p className="ct-detail__prose m-0">{node.evidence.summary}</p>}
          {time && <p className="ct-job-note">{time}</p>}
        </div>
      </section>
      : model.mode === "unavailable" && <p role="status" className="ct-job-note">Per-node evidence is unavailable for this run.</p>}
    {definition?.does && <p className="ct-detail__prose m-0">{definition.does}</p>}
    {node.body && !definition?.does && node.body.kind === "text" && <p className="ct-detail__prose m-0">{node.body.text}</p>}
    {definition && <Facts>
      <dt>Node ID</dt><dd className="ct-detail__mono">{definition.id}</dd>
      <dt>Kind</dt><dd>{node.kindLabel}{definition.uses ? ` (${definition.uses})` : ""}</dd>
      {definition.in && <><dt>Input</dt><dd>{portType(definition.in)}</dd></>}
      {definition.out && <><dt>Output</dt><dd>{portType(definition.out)}</dd></>}
      {node.inputs.filter((pin) => pin.id.startsWith("bind:")).map((pin) => <Fragment key={pin.id}><dt>{pin.label}</dt><dd className="ct-detail__mono">from {pin.detail}</dd></Fragment>)}
    </Facts>}
    {node.body?.kind === "prompt" && <section aria-label="Instruction" className="ct-detail__block"><h4 className={SECTION}>Instruction</h4><p className="ct-job-quote">{node.body.text}</p></section>}
    {node.body?.kind === "code" && <section aria-label="Code body" className="ct-detail__block"><h4 className={SECTION}>{node.kindLabel === "Command" || node.kindLabel === "Probe" ? "Command" : "Code"}</h4><CodeBlock text={node.body.text} language={node.body.language} wrapLines
      fullscreenAside={definition ? <CodeAside definition={definition} language={node.body.language} languageLabel={node.kindLabel} source={node.body.text} /> : undefined} /></section>}
    {node.routes.length > 0 && <section aria-label="Routes" className="ct-detail__block">
      <h4 className={SECTION}>Routes</h4>
      <dl className="bp-routes">{node.routes.map((route) => <Fragment key={route.outcome}><dt>{route.outcome}</dt><dd>{route.target === "$stop" ? "Stop" : route.target === "$result" ? "Result" : titles.get(route.target) ?? route.target}</dd></Fragment>)}</dl>
    </section>}
    {call && <section aria-label={`Callee ${call.pin.id}`} className="ct-detail__block">
      <h4 className={SECTION}>Sub-graph{node.callee ? ` · ${node.callee.name}` : ""}</h4>
      <div className="flex flex-wrap items-center gap-2">
        {onOpen && <Button variant="primary" size="sm" onClick={onOpen}>Open sub-graph</Button>}
        <Button variant="ghost" size="sm" aria-expanded={expanded} onClick={onExpand}>{expanded ? "Collapse callee" : "Expand callee"}</Button>
      </div>
      <Facts><dt>Solution</dt><dd className="ct-detail__mono">{call.pin.id}</dd><dt>Content hash</dt><dd className="ct-detail__mono">{call.pin.hash}</dd></Facts>
      <BlueprintProjectionValue value={call.graph.inputs ?? {}} label="Callee inputs" />
      <BlueprintProjectionValue value={call.graph.result} label="Callee result" />
    </section>}
    {definition?.call && !call && <p role="status" className="ct-job-note">Expanded callee unavailable.</p>}
    {node.evidence?.journal && <BlueprintProjectionValue value={node.evidence.journal} label="Recorded node result" />}
    {definition && <Disclosure title="Definition">
      <div className="flex flex-col gap-2">
        {definition.in && <BlueprintProjectionValue value={definition.in} label="Input port" />}
        {definition.out && <BlueprintProjectionValue value={definition.out} label="Output port" />}
        <BlueprintProjectionValue value={definition} label="Node definition" />
      </div>
    </Disclosure>}
  </DetailsFrame>;
}

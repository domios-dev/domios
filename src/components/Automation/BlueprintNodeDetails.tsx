import { Fragment, type CSSProperties, type RefObject } from "react";
import { CodeBlock } from "@codeterm/chat-ui";
import type { PresenceState } from "../Canvas/usePresence";
import { inspectBlueprintNode } from "./blueprintInspector";
import type { BlueprintNode } from "./blueprintModel";
import { nodeIcon } from "./BlueprintNodeCard";
import { CommandResultFacts } from "./CommandResultDetails";
import { DetailsFrame, Disclosure, Facts } from "./DetailsFrame";
import { NodeFiles } from "./NodeFiles";
import type { AutomationGraph } from "./types";

const SECTION = "ct-detail__heading";

export function BlueprintNodeDetails({ node, graph, onClose, panelRef, presence }: {
  node: BlueprintNode | null; graph: AutomationGraph; onClose: () => void;
  panelRef?: RefObject<HTMLElement | null>; presence?: PresenceState;
}) {
  if (!node) return <DetailsFrame label="Details: unavailable node" title={<h3 className="plan-side-panel__title">Node unavailable</h3>} onClose={onClose} panelRef={panelRef} presence={presence} focusWhenLost>
    <p role="status" className="ct-detail__empty">The selected node is no longer part of this Blueprint.</p>
  </DetailsFrame>;
  const view = inspectBlueprintNode(node, graph);
  const Icon = nodeIcon(node);
  return <DetailsFrame label={`Details: ${view.name}`} onClose={onClose} panelRef={panelRef} presence={presence} title={<div className="bp-scope flex min-w-0 items-center gap-3">
    <span className="ct-card__icon" style={{ "--ct-card-tone": `var(--bp-${node.type})` } as CSSProperties}><Icon size={14} aria-hidden="true" /></span>
    <span className="min-w-0">
      <h3 className="plan-side-panel__title">{view.name}</h3>
      <span className="text-xs text-[var(--ct-muted-fg)]">{view.type} node · {node.stage.name}</span>
    </span>
  </div>}>
    <p className="ct-detail__prose m-0">{view.purpose}</p>
    {view.properties.length > 0 && <Facts>{view.properties.map(([name, value]) => <Fragment key={name}><dt>{name}</dt><dd>{value}</dd></Fragment>)}</Facts>}
    {view.prompt && <section aria-label="Instruction" className="ct-detail__block">
      <h4 className={SECTION}>Instruction</h4>
      <p className="ct-job-quote">{view.prompt}</p>
    </section>}
    {view.routes.length > 0 && <section aria-label="Routes" className="ct-detail__block">
      <h4 className={SECTION}>Routes</h4>
      <dl className="ct-detail__sections m-0">{view.routes.map((route) => <div key={route.name} className="ct-detail__section"><dt className="font-semibold">{route.name}</dt><dd className="ct-detail__prose m-0">{route.description}</dd></div>)}</dl>
    </section>}
    {view.code && <section aria-label="Node code" className="ct-detail__block">
      <h4 className={SECTION}>{node.type === "source" ? "Probe command" : "Command"}</h4>
      <CodeBlock text={view.code.text} language={view.code.language} fullscreenScope="viewport" wrapLines />
      {view.code.truncated && <p className="ct-job-note">Code preview truncated at the source.</p>}
    </section>}
    {view.command && <section aria-label="Last command result" className="ct-detail__block"><h4 className={SECTION}>Last result</h4><CommandResultFacts handlerId={view.command.handler.id} handler={view.command.handler} result={view.command.result} /></section>}
    {view.files && <NodeFiles key={JSON.stringify([view.files, node.stage.blueprint_definition?.definition_version])} identity={view.files.identity} bucket={view.files.bucket} handler={view.files.handler} />}
    <Disclosure title="Identifiers"><Facts><dt>Stage</dt><dd className="ct-detail__mono">{node.stage.identity.id}</dd>{node.action?.handler && <><dt>Action</dt><dd className="ct-detail__mono">{node.action.handler.id}</dd></>}</Facts></Disclosure>
  </DetailsFrame>;
}

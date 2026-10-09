import { Fragment, type RefObject } from "react";
import { CodeBlock } from "@codeterm/chat-ui";
import type { PresenceState } from "../Canvas/usePresence";
import { inspectBlueprintNode } from "./blueprintInspector";
import type { BlueprintNode } from "./blueprintModel";
import { nodeIcon } from "./BlueprintNodeCard";
import { CommandResultFacts } from "./CommandResultDetails";
import { DetailsFrame, Disclosure, Facts } from "./DetailsFrame";
import { NodeFiles } from "./NodeFiles";
import type { AutomationGraph } from "./types";

const SECTION = "text-[length:var(--ct-font-size-sm)] font-semibold text-[var(--ct-fg)]";

export function BlueprintNodeDetails({ node, graph, onClose, panelRef, presence }: {
  node: BlueprintNode | null; graph: AutomationGraph; onClose: () => void;
  panelRef?: RefObject<HTMLElement | null>; presence?: PresenceState;
}) {
  if (!node) return <DetailsFrame label="Details: unavailable node" title={<h3>Node unavailable</h3>} onClose={onClose} panelRef={panelRef} presence={presence} focusWhenLost>
    <p role="status">The selected node is no longer part of this Blueprint.</p>
  </DetailsFrame>;
  const view = inspectBlueprintNode(node, graph);
  const Icon = nodeIcon(node);
  return <DetailsFrame label={`Details: ${view.name}`} onClose={onClose} panelRef={panelRef} presence={presence} title={<div className="bp-scope flex min-w-0 items-center gap-3">
    <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg" style={{ background: `color-mix(in srgb, var(--bp-${node.type}) 22%, transparent)`, color: `var(--bp-${node.type})` }}><Icon size={16} aria-hidden="true" /></span>
    <span className="min-w-0">
      <h3 className="truncate text-[length:var(--ct-font-size-lg)] font-semibold">{view.name}</h3>
      <span className="text-xs text-[var(--ct-muted-fg)]">{view.type} node · {node.stage.name}</span>
    </span>
  </div>}>
    <div className="space-y-5">
      <p className="leading-relaxed">{view.purpose}</p>
      {view.properties.length > 0 && <Facts>{view.properties.map(([name, value]) => <Fragment key={name}><dt>{name}</dt><dd>{value}</dd></Fragment>)}</Facts>}
      {view.prompt && <section aria-label="Instruction" className="space-y-2">
        <h4 className={SECTION}>Instruction</h4>
        <p className="whitespace-pre-wrap break-words rounded-lg border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-3 py-2.5 leading-relaxed">{view.prompt}</p>
      </section>}
      {view.routes.length > 0 && <section aria-label="Routes" className="space-y-2">
        <h4 className={SECTION}>Routes</h4>
        <dl className="space-y-2">{view.routes.map((route) => <div key={route.name}><dt className="font-medium">{route.name}</dt><dd className="text-[var(--ct-muted-fg)]">{route.description}</dd></div>)}</dl>
      </section>}
      {view.code && <section aria-label="Node code" className="space-y-2">
        <h4 className={SECTION}>{node.type === "source" ? "Probe command" : "Command"}</h4>
        <CodeBlock text={view.code.text} language={view.code.language} fullscreenScope="viewport" wrapLines />
        {view.code.truncated && <p className="text-xs text-[var(--ct-muted-fg)]">Code preview truncated at the source.</p>}
      </section>}
      {view.command && <section aria-label="Last command result"><h4 className={SECTION}>Last result</h4><CommandResultFacts handlerId={view.command.handler.id} handler={view.command.handler} result={view.command.result} /></section>}
      {view.files && <NodeFiles key={JSON.stringify([view.files, node.stage.sentinel_definition?.definition_version])} identity={view.files.identity} bucket={view.files.bucket} handler={view.files.handler} />}
      <Disclosure title="Identifiers"><Facts><dt>Stage</dt><dd className="break-all font-mono text-xs">{node.stage.identity.id}</dd>{node.action?.handler && <><dt>Action</dt><dd className="break-all font-mono text-xs">{node.action.handler.id}</dd></>}</Facts></Disclosure>
    </div>
  </DetailsFrame>;
}

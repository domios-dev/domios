import type { BlueprintExpandedCall } from "../../lib/blueprintApi";
import type { PipelineViewNode } from "./pipelineModel";
import { BlueprintProjectionValue } from "./BlueprintProjectionValue";
import { Button } from "../shared/Button";

export function BlueprintPipelineNodeDetails({ node, call, expanded, onExpand, onClose }: {
  node: PipelineViewNode; call?: BlueprintExpandedCall; expanded: boolean; onExpand: () => void; onClose: () => void;
}) {
  return <section aria-label={`Details: ${node.title}`} className="ct-card ct-card--static bp-panel">
    <div className="flex items-center gap-2">
      <h3 className="bp-panel__title min-w-0 flex-1 truncate">{node.title}</h3>
      <Button variant="ghost" size="sm" onClick={onClose} aria-label="Close node details">Close</Button>
    </div>
    {node.definition && <>
      <dl className="bp-facts"><dt>Node ID</dt><dd>{node.definition.id}</dd><dt>Kind</dt><dd>{node.capability}</dd></dl>
      {node.definition.in && <BlueprintProjectionValue value={node.definition.in} label="Input port" />}
      {node.definition.out && <BlueprintProjectionValue value={node.definition.out} label="Output port" />}
      {node.definition.code && <pre aria-label="Code body" className="bp-mono whitespace-pre-wrap break-words">{node.definition.code.src ?? node.definition.code.file}</pre>}
      <BlueprintProjectionValue value={node.definition} label="Node definition" />
    </>}
    {call && <section aria-label={`Callee ${call.pin.id}`} className="flex flex-col gap-2">
      <dl className="bp-facts"><dt>Solution</dt><dd>{call.pin.id}</dd><dt>Content hash</dt><dd className="bp-mono">{call.pin.hash}</dd></dl>
      <div><Button variant="ghost" size="sm" aria-expanded={expanded} onClick={onExpand}>{expanded ? "Collapse callee" : "Expand callee"}</Button></div>
      <BlueprintProjectionValue value={call.graph.inputs ?? {}} label="Callee inputs" />
      <BlueprintProjectionValue value={call.graph.result} label="Callee result" />
      <BlueprintProjectionValue value={call.graph} label="Expanded callee body" />
    </section>}
    {node.definition?.call && !call && <p role="status" className="ct-job-note">Expanded callee unavailable.</p>}
    {node.journal && <BlueprintProjectionValue value={node.journal} label="Recorded node result" />}
  </section>;
}

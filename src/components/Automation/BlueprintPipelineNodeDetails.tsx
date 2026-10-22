import type { BlueprintExpandedCall } from "../../lib/blueprintApi";
import type { PipelineViewNode } from "./pipelineModel";
import { BlueprintProjectionValue } from "./BlueprintProjectionValue";

export function BlueprintPipelineNodeDetails({ node, call, expanded, onExpand, onClose }: {
  node: PipelineViewNode; call?: BlueprintExpandedCall; expanded: boolean; onExpand: () => void; onClose: () => void;
}) {
  return <section aria-label={`Details: ${node.title}`} className="ct-detail__block">
    <button type="button" onClick={onClose} aria-label="Close node details">Close</button>
    <h3>{node.title}</h3>
    {node.definition && <>
      <dl><dt>Node ID</dt><dd>{node.definition.id}</dd><dt>Kind</dt><dd>{node.capability}</dd></dl>
      {node.definition.in && <BlueprintProjectionValue value={node.definition.in} label="Input port" />}
      {node.definition.out && <BlueprintProjectionValue value={node.definition.out} label="Output port" />}
      {node.definition.code && <pre aria-label="Code body" className="whitespace-pre-wrap break-words">{node.definition.code.src ?? node.definition.code.file}</pre>}
      <BlueprintProjectionValue value={node.definition} label="Node definition" />
    </>}
    {call && <section aria-label={`Callee ${call.pin.id}`}>
      <dl><dt>Solution</dt><dd>{call.pin.id}</dd><dt>Content hash</dt><dd>{call.pin.hash}</dd></dl>
      <button type="button" aria-expanded={expanded} onClick={onExpand}>{expanded ? "Collapse callee" : "Expand callee"}</button>
      <BlueprintProjectionValue value={call.graph.inputs ?? {}} label="Callee inputs" />
      <BlueprintProjectionValue value={call.graph.result} label="Callee result" />
      <BlueprintProjectionValue value={call.graph} label="Expanded callee body" />
    </section>}
    {node.definition?.call && !call && <p role="status">Expanded callee unavailable.</p>}
    {node.journal && <BlueprintProjectionValue value={node.journal} label="Recorded node result" />}
  </section>;
}

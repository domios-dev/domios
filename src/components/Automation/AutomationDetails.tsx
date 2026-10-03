import { useLayoutEffect, useRef } from "react";
import { AUTOMATION_BUTTON } from "./styles";
import { automationState } from "./model";
import { automationKey, type AutomationEdge, type AutomationNode } from "./types";

export function AutomationDetails({ node, edges, nodes, stale, stopping, stopFailed, onStop, onSelect, onClose }: {
  node: AutomationNode; edges: readonly AutomationEdge[]; nodes: readonly AutomationNode[];
  stale: boolean; stopping: boolean; stopFailed: boolean;
  onStop: () => void; onSelect: (node: AutomationNode) => void; onClose: () => void;
}) {
  const panel = useRef<HTMLElement>(null);
  const key = automationKey(node.identity);
  useLayoutEffect(() => { panel.current?.focus(); }, [key]);
  const links = edges.filter((edge) => automationKey(edge.source) === key || automationKey(edge.target) === key);
  return <aside ref={panel} tabIndex={-1} role="region" aria-label={`Details: ${node.name}`} className="border-t border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] p-4 text-sm text-[var(--ct-fg)] outline-none" onKeyDown={(event) => { if (event.code === "Escape") onClose(); }}>
    <div className="flex items-center justify-between gap-3"><h3 className="font-semibold">{node.name}</h3><button type="button" className={AUTOMATION_BUTTON} onClick={onClose}>Close details</button></div>
    <dl className="my-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 break-words">
      <dt>Purpose / question / task</dt><dd>{node.purpose ?? "Unknown"}</dd>
      <dt>Status</dt><dd>{automationState(node)}{stale ? " · Stale" : ""}{node.hidden === null ? " · Visibility unknown" : node.hidden ? " · Hidden" : ""}</dd>
      <dt>Executor kind</dt><dd>{node.executor?.kind ?? "Unknown"}</dd>
      <dt>Provider</dt><dd>{node.executor?.provider_id ?? "Unknown"}</dd>
      <dt>Decision adapter</dt><dd>{node.executor?.adapter_id ?? "Unknown"}</dd>
      <dt>Model</dt><dd>{node.executor?.model ?? "Unknown"}</dd>
      <dt>Capabilities</dt><dd>{node.executor?.capabilities?.join(" · ") ?? "Unknown"}</dd>
      <dt>ID</dt><dd>{node.identity.id}</dd>
      <dt>Owner</dt><dd>{node.identity.owner_tab_id ?? "Unknown"}</dd>
      <dt>Workspace</dt><dd>{node.identity.workspace_id}</dd>
      <dt>Team</dt><dd>{node.identity.team_id ?? "No Team"}</dd>
      <dt>Schedule</dt><dd>{node.schedule ?? "Not recorded"}</dd>
      <dt>Cadence</dt><dd>{node.cadence_ms === null ? "Not recorded" : `${node.cadence_ms} ms`}</dd>
      <dt>Expiry</dt><dd>{node.expires_at === null ? "Not recorded" : new Date(node.expires_at).toLocaleString()}</dd>
      <dt>Last accepted result</dt><dd>{node.last_result ? `${node.last_result.bucket} · ${new Date(node.last_result.accepted_at).toLocaleString()}` : "None recorded"}</dd>
    </dl>
    {node.last_result && <ul aria-label="Last accepted handler results">{node.last_result.handlers.map((result) => <li key={result.handler_id}>{result.handler_id} · {result.status ?? "Unknown"} · {result.execution_id ?? "No execution recorded"}</li>)}</ul>}
    <h4 className="font-medium">Possible outcomes</h4>
    {node.outcomes.length ? <ul>{node.outcomes.map((outcome, index) => <li key={index}>{outcome.bucket}<ul>{outcome.handlers.map((handler) => <li key={handler.id}>{handler.id} · {handler.type} · {handler.availability} · {handler.target?.id ?? "No target"} · {handler.last_result?.status ?? "No result recorded"}{handler.last_result?.execution_id && ` · ${handler.last_result.execution_id}`}</li>)}</ul></li>)}</ul> : <p>None recorded</p>}
    <h4 className="mt-3 font-medium">Links</h4>
    {links.length ? <ul className="space-y-2">{links.map((edge) => {
      const targetIdentity = automationKey(edge.source) === key ? edge.target : edge.source;
      const target = nodes.find((candidate) => automationKey(candidate.identity) === automationKey(targetIdentity));
      return <li key={edge.id}><span>{edge.evidence.kind} · {edge.bucket} · {edge.handler_type} · {edge.handler_id} · {edge.result?.status ?? "No result recorded"} · {edge.evidence.kind === "configured" ? edge.evidence.route_id : edge.evidence.execution_id} → </span>{target && <button type="button" className={AUTOMATION_BUTTON} onClick={() => onSelect(target)}>{target.name} · {automationState(target)}</button>}</li>;
    })}</ul> : <p>None recorded</p>}
    {node.identity.kind === "sentinel" && <div className="mt-4">
      <button type="button" className={AUTOMATION_BUTTON} disabled={!node.stop.allowed || stale || stopping} onClick={onStop}>{stopping ? "Stopping…" : "Stop Sentinel"}</button>
      {!node.stop.allowed && node.stop.reason && <p>{node.stop.reason}</p>}
      {stopping && <p role="status">Stop requested; waiting for authoritative state.</p>}
      {stopFailed && <p role="alert">Sentinel stop failed. State has not been changed by this view.</p>}
    </div>}
  </aside>;
}

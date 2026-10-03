import { useLayoutEffect, useRef, type ReactNode } from "react";
import { X } from "lucide-react";
import { focusLost } from "./focus";
import { AUTOMATION_BUTTON, AUTOMATION_LABEL, AUTOMATION_TOOL, stateTone } from "./styles";
import { automationState } from "./model";
import { automationKey, type AutomationEdge, type AutomationNode } from "./types";

function Group({ title, children }: { title: string; children: ReactNode }) {
  return <section className="min-w-0"><h4 className={`${AUTOMATION_LABEL} mb-2`}>{title}</h4>{children}</section>;
}

function Facts({ children }: { children: ReactNode }) {
  return <dl className="grid grid-cols-[minmax(6.5rem,auto)_1fr] gap-x-4 gap-y-1.5 break-words [&>dt]:text-[var(--ct-muted)] [&>dd]:min-w-0 [&>dd]:text-[var(--ct-fg)]">{children}</dl>;
}

function Status({ value }: { value: string }) {
  return <span className="inline-flex items-center gap-1.5 rounded-full border border-[var(--ct-border)] px-2 py-0.5 text-xs text-[var(--ct-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(value)}`} aria-hidden="true" />{value}</span>;
}

function seconds(ms: number): string {
  return ms % 1000 === 0 ? `${ms / 1000}s` : `${(ms / 1000).toFixed(1)}s`;
}

function DetailsFrame({ label, title, onClose, children, focusWhenLost = false }: { label: string; title: ReactNode; onClose: () => void; children: ReactNode; focusWhenLost?: boolean }) {
  const panel = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    if (!focusWhenLost || focusLost()) panel.current?.focus();
  }, [label, focusWhenLost]);
  return <aside ref={panel} tabIndex={-1} role="region" aria-label={label} className="border-t border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-4 text-[length:var(--ct-font-size-sm)] text-[var(--ct-fg)] outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]" onKeyDown={(event) => {
    if (event.code !== "Escape") return;
    event.stopPropagation();
    onClose();
  }}>
    <div className="mb-4 flex items-start justify-between gap-3">{title}<button type="button" className={AUTOMATION_TOOL} aria-label="Close details" title="Close details (Esc)" onClick={onClose}><X size={14} aria-hidden="true" /></button></div>
    {children}
  </aside>;
}

export function AutomationDetailsUnavailable({ onClose }: { onClose: () => void }) {
  return <DetailsFrame label="Details: unavailable participant" title={<h3 className="text-[length:var(--ct-font-size-md)] font-semibold">Participant unavailable</h3>} onClose={onClose} focusWhenLost>
    <p role="status" className="text-[var(--ct-muted-fg)]">The selected participant is no longer part of this scenario's authoritative graph. Select another participant to continue.</p>
  </DetailsFrame>;
}

export function AutomationDetails({ node, edges, nodes, stale, stopping, stopFailed, onStop, onSelect, onClose }: {
  node: AutomationNode; edges: readonly AutomationEdge[]; nodes: readonly AutomationNode[];
  stale: boolean; stopping: boolean; stopFailed: boolean;
  onStop: () => void; onSelect: (node: AutomationNode) => void; onClose: () => void;
}) {
  const key = automationKey(node.identity);
  const links = edges.filter((edge) => automationKey(edge.source) === key || automationKey(edge.target) === key);
  const state = automationState(node);
  const title = <div className="min-w-0">
    <p className={AUTOMATION_LABEL}>{node.identity.kind}</p>
    <h3 className="truncate text-[length:var(--ct-font-size-lg)] font-semibold">{node.name}</h3>
  </div>;
  return <DetailsFrame label={`Details: ${node.name}`} title={title} onClose={onClose}>
    <div className="@container"><div className="grid gap-5 @3xl:grid-cols-2">
      <Group title="Overview">
        <p className="mb-3 whitespace-pre-wrap leading-relaxed">{node.purpose ?? "Unknown"}</p>
        <Facts>
          <dt>Status</dt><dd className="flex flex-wrap items-center gap-1.5"><Status value={state} />{stale ? " · Stale" : ""}{node.hidden === null ? " · Visibility unknown" : node.hidden ? " · Hidden" : ""}</dd>
          <dt>Schedule</dt><dd className="font-mono text-xs">{node.schedule ?? "Not recorded"}</dd>
          <dt>Cadence</dt><dd>{node.cadence_ms === null ? "Not recorded" : `${seconds(node.cadence_ms)} (${node.cadence_ms} ms)`}</dd>
          <dt>Expiry</dt><dd>{node.expires_at === null ? "Not recorded" : new Date(node.expires_at).toLocaleString()}</dd>
        </Facts>
      </Group>
      <Group title="Executor and identity">
        <Facts>
          <dt>Executor kind</dt><dd>{node.executor?.kind ?? "Unknown"}</dd>
          <dt>Provider</dt><dd className="font-mono text-xs">{node.executor?.provider_id ?? "Unknown"}</dd>
          <dt>Decision adapter</dt><dd className="font-mono text-xs">{node.executor?.adapter_id ?? "Unknown"}</dd>
          <dt>Model</dt><dd className="font-mono text-xs">{node.executor?.model ?? "Unknown"}</dd>
          <dt>Capabilities</dt><dd>{node.executor?.capabilities?.join(" · ") ?? "Unknown"}</dd>
          <dt>ID</dt><dd className="font-mono text-xs">{node.identity.id}</dd>
          {node.identity.kind === "sentinel" && <><dt>Creator provenance</dt><dd>{node.sentinel_definition ? `${node.sentinel_definition.source} · ${node.sentinel_definition.creator_tab_id}` : "Unknown"}</dd></>}
          <dt>Owner</dt><dd className="font-mono text-xs">{node.identity.owner_tab_id ?? "Unknown"}</dd>
          <dt>Workspace</dt><dd className="font-mono text-xs">{node.identity.workspace_id}</dd>
          <dt>Team</dt><dd>{node.identity.team_id ?? "No Team"}</dd>
        </Facts>
      </Group>
      <Group title="Last accepted result">
        {node.last_result ? <>
          <p className="mb-2">{node.last_result.bucket} · {new Date(node.last_result.accepted_at).toLocaleString()}</p>
          <ul aria-label="Last accepted handler results" className="space-y-1.5">{node.last_result.handlers.map((result) => <li key={result.handler_id} className="flex flex-wrap items-center gap-x-2 gap-y-1 rounded-md border border-[var(--ct-border)] px-2.5 py-1.5">
            <span className="font-medium">{result.handler_id}</span><span aria-hidden="true"> · </span><Status value={result.status ?? "Unknown"} /><span aria-hidden="true"> · </span><span className="font-mono text-xs text-[var(--ct-muted-fg)]">{result.execution_id ?? "No execution recorded"}</span>
          </li>)}</ul>
        </> : <p className="text-[var(--ct-muted-fg)]">None recorded</p>}
      </Group>
      <Group title="Possible outcomes">
        {node.outcomes.length ? <ul className="space-y-2.5">{node.outcomes.map((outcome, index) => <li key={index}>
          <p className="font-medium">{outcome.bucket}</p>
          <ul className="mt-1 space-y-1 border-l border-[var(--ct-border)] pl-3">{outcome.handlers.map((handler) => <li key={handler.id} className="text-[var(--ct-muted-fg)]">
            <span className="text-[var(--ct-fg)]">{handler.id} · {handler.type} · {handler.availability}</span> · {handler.target?.id ?? "No target"} · {handler.last_result?.status ?? "No result recorded"}{handler.last_result?.execution_id && <span className="font-mono text-xs"> · {handler.last_result.execution_id}</span>}
          </li>)}</ul>
        </li>)}</ul> : <p className="text-[var(--ct-muted-fg)]">None recorded</p>}
      </Group>
      <div className="@3xl:col-span-2"><Group title="Links">
        {links.length ? <ul className="space-y-2">{links.map((edge) => {
          const outgoing = automationKey(edge.source) === key;
          const targetIdentity = outgoing ? edge.target : edge.source;
          const target = nodes.find((candidate) => automationKey(candidate.identity) === automationKey(targetIdentity));
          return <li key={edge.id} className="flex flex-wrap items-center gap-2 rounded-md border border-[var(--ct-border)] px-2.5 py-2">
            <span className={`rounded-full px-2 py-0.5 text-[0.6875rem] ${edge.evidence.kind === "observed" ? "bg-[color-mix(in_srgb,var(--ct-fg)_12%,transparent)] text-[var(--ct-fg)]" : "border border-dashed border-[var(--ct-border)] text-[var(--ct-muted-fg)]"}`}>{edge.evidence.kind}</span>
            <span className="min-w-0 flex-1 break-words"> · {edge.bucket} · {edge.handler_type} · {edge.handler_id} · {edge.result?.status ?? "No result recorded"} · <span className="font-mono text-xs">{edge.evidence.kind === "configured" ? edge.evidence.route_id : edge.evidence.execution_id}</span> {outgoing ? "→" : "←"} </span>
            {target && <button type="button" className={AUTOMATION_BUTTON} onClick={() => onSelect(target)}>{target.name} · {automationState(target)}</button>}
          </li>;
        })}</ul> : <p className="text-[var(--ct-muted-fg)]">None recorded</p>}
      </Group></div>
    </div></div>
    {node.identity.kind === "sentinel" && <div className="mt-5 flex flex-wrap items-center gap-3 border-t border-[var(--ct-border)] pt-4">
      <button type="button" className={`${AUTOMATION_BUTTON} disabled:opacity-50`} disabled={!node.stop.allowed || stale || stopping} onClick={onStop}>{stopping ? "Stopping…" : "Stop Sentinel"}</button>
      {!node.stop.allowed && node.stop.reason && <p className="text-[var(--ct-muted-fg)]">{node.stop.reason}</p>}
      {stopping && <p role="status">Stop requested; waiting for authoritative state.</p>}
      {stopFailed && <p role="alert">Sentinel stop failed. State has not been changed by this view.</p>}
    </div>}
  </DetailsFrame>;
}

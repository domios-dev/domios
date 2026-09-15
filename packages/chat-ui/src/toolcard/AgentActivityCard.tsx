import { agentActionStream, type AgentActivityState, type AgentNode, type ToolEvent } from "@codeterm/chat-core";
import { ArrowRight, ChevronRight, Network } from "lucide-react";
import { Fragment, useId, useState } from "react";
import { ToolActivityStream, type RenderStreamAction } from "./ToolActivityStream";

const STATE_COLOR: Record<AgentActivityState, string> = {
  started: "var(--ct-yellow)",
  running: "var(--ct-yellow)",
  completed: "var(--ct-green)",
  failed: "var(--ct-red)",
  interrupted: "var(--ct-muted-fg)",
  closed: "var(--ct-muted-fg)",
  unknown: "var(--ct-muted-fg)",
};

function plural(count: number, noun: string): string {
  return count === 0 ? "" : `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function compactCount(value: number): string {
  return value >= 1000 ? `${(value / 1000).toFixed(1)}k` : String(value);
}

function duration(ms: number): string {
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

/** Only facts the source reported: model, duration, tool uses, tokens. */
export function agentFacts(agent: AgentNode): string[] {
  const facts: string[] = [];
  if (agent.model) facts.push(agent.model);
  if (agent.durationMs !== undefined) facts.push(duration(agent.durationMs));
  if (agent.toolUses !== undefined) facts.push(`${agent.toolUses} tool use${agent.toolUses === 1 ? "" : "s"}`);
  if (agent.tokens !== undefined) facts.push(`${compactCount(agent.tokens)} tokens`);
  return facts;
}

export function AgentActivityCard({ event, expanded, onToggle, renderAction }: {
  event: ToolEvent;
  expanded: boolean;
  onToggle: (next: boolean) => void;
  renderAction: RenderStreamAction;
}) {
  const regionId = useId();
  const [collapsedStreams, setCollapsedStreams] = useState<ReadonlySet<string>>(() => new Set());
  const tree = event.agents;
  if (!tree) return null;
  const counts = [plural(tree.agents.length, "agent"), plural(tree.messages.length, "message")].filter(Boolean).join(" · ");
  const described = tree.agents.length === 1 && tree.messages.length === 0 && tree.agents[0].description ? tree.agents[0] : undefined;
  const toggleStream = (key: string, open: boolean) => setCollapsedStreams((previous) => {
    const next = new Set(previous);
    if (open) next.delete(key);
    else next.add(key);
    return next;
  });

  return (
    <section
      data-testid="chat-agent-activity"
      aria-label={`Agent activity, ${counts}`}
      className="mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-md border-l-2"
      style={{ borderLeftColor: "var(--ct-border)", background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)" }}
    >
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={regionId}
        onClick={() => onToggle(!expanded)}
        className="flex w-full min-w-0 items-center gap-2 px-3 py-1.5 text-left text-xs focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]"
      >
        <Network aria-hidden="true" size={12} className="shrink-0" style={{ color: "var(--ct-accent)" }} />
        <span data-testid="chat-agent-title" className="min-w-0 flex-1 truncate font-mono text-[12px]" style={{ color: "var(--ct-fg)" }}>
          {described?.description ?? "Agents"}
        </span>
        {described ? (
          <>
            <span data-testid="chat-agent-facts" className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>
              {agentFacts(described).join(" · ")}
            </span>
            <span data-testid="chat-agent-header-state" className="shrink-0 font-mono text-[10.5px]" style={{ color: STATE_COLOR[described.state] }}>
              {described.state}
            </span>
          </>
        ) : (
          <span data-testid="chat-agent-counts" className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{counts}</span>
        )}
        <ChevronRight
          aria-hidden="true"
          size={14}
          className={`shrink-0 motion-safe:transition-transform motion-reduce:transition-none ${expanded ? "rotate-90" : ""}`}
          style={{ color: "var(--ct-muted-fg)" }}
        />
      </button>
      <div id={regionId} role="region" aria-label="Agent activity details" hidden={!expanded} className="px-3 pb-2">
        {expanded && tree.agents.length > 0 && (
          <ul aria-label="Agents" className="m-0 list-none p-0">
            {tree.agents.map((agent) => {
              const facts = agentFacts(agent);
              const stream = agentActionStream(agent, event.message);
              return (
                <Fragment key={agent.key}>
                  <li
                    data-testid="chat-agent-row"
                    data-agent-state={agent.state}
                    className="flex min-w-0 items-center gap-2 py-0.5 font-mono text-[11.5px]"
                    style={{ paddingLeft: `${agent.depth * 12}px` }}
                  >
                    <span data-testid="chat-agent-name" className="shrink-0" style={{ color: "var(--ct-fg)" }}>{agent.name}</span>
                    <span data-testid="chat-agent-path" className="min-w-0 flex-1 truncate text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>
                      {agent.path ?? agent.id}
                    </span>
                    {facts.length > 0 && (
                      <span data-testid="chat-agent-row-facts" className="shrink-0 text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{facts.join(" · ")}</span>
                    )}
                    <span data-testid="chat-agent-state" className="shrink-0 text-[10.5px]" style={{ color: STATE_COLOR[agent.state] }}>{agent.state}</span>
                  </li>
                  {(stream || agent.result) && (
                    <li data-testid="chat-agent-detail" className="min-w-0 py-1" style={{ paddingLeft: `${agent.depth * 12}px` }}>
                      {stream && (
                        <ToolActivityStream
                          event={stream}
                          expanded={!collapsedStreams.has(agent.key)}
                          onToggle={(open) => toggleStream(agent.key, open)}
                          renderAction={renderAction}
                        />
                      )}
                      {agent.result && (
                        <div
                          data-testid="chat-agent-result"
                          aria-label="Agent result"
                          className="whitespace-pre-wrap break-words font-mono text-[11px]"
                          style={{ color: "var(--ct-fg)" }}
                        >
                          {agent.result}
                        </div>
                      )}
                    </li>
                  )}
                </Fragment>
              );
            })}
          </ul>
        )}
        {expanded && tree.messages.length > 0 && (
          <ul aria-label="Messages" className="m-0 mt-1 list-none p-0">
            {tree.messages.map((edge) => (
              <li
                key={JSON.stringify([edge.from, edge.to])}
                data-testid="chat-agent-message"
                className="flex min-w-0 items-center gap-1.5 py-0.5 font-mono text-[11px]"
                style={{ color: "var(--ct-muted-fg)" }}
              >
                <span data-testid="chat-agent-message-from" className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>{edge.fromLabel}</span>
                <ArrowRight aria-hidden="true" size={11} className="shrink-0" />
                <span className="sr-only">messaged</span>
                <span data-testid="chat-agent-message-to" className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>{edge.toLabel}</span>
                {edge.count > 1 && <span className="shrink-0 text-[10.5px]">×{edge.count}</span>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}

import type { CSSProperties } from "react";
import { PRESENCE_INK, agentInitials, type PresenceAgent } from "./identity";

export interface AgentCircleProps {
  agent: PresenceAgent;
  size?: number;
  live?: boolean;
  /** bg-colored separator ring — set false when the circle sits on its own. */
  ring?: boolean;
  title?: string;
}

function tokenTitle(agent: PresenceAgent): string {
  return [agent.label, agent.role ? `· ${agent.role}` : null, agent.isPeer ? "(peer)" : null]
    .filter(Boolean)
    .join(" ");
}

export function AgentCircle({ agent, size = 18, live = false, ring = true, title }: AgentCircleProps) {
  return (
    <span
      data-testid="canvas-agent-circle"
      data-agent-id={agent.id}
      className={live ? "ct-presence-live" : undefined}
      title={title ?? tokenTitle(agent)}
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        boxSizing: "border-box",
        background: agent.color,
        color: PRESENCE_INK,
        border: ring ? "1.5px solid var(--ct-bg)" : "none",
        boxShadow: `0 0 0 1px color-mix(in srgb, ${agent.color} 65%, transparent)`,
        fontSize: Math.round(size * 0.42),
        fontWeight: 800,
        lineHeight: 1,
        letterSpacing: "0.01em",
        fontFamily: "var(--ct-font-ui)",
        userSelect: "none",
        // expose the agent color so the live keyframe can ring in-identity.
        ["--ct-presence-color" as string]: agent.color,
      }}
    >
      {agentInitials(agent.label)}
    </span>
  );
}

export interface AgentStackProps {
  agents: PresenceAgent[];
  size?: number;
  /** Max circles before collapsing the tail into a +N token. */
  max?: number;
  /** Pulse the lead (primary executor) circle. */
  live?: boolean;
}

// tucks left under it; overflow collapses to a neutral +N token.
export function AgentStack({ agents, size = 18, max = 3, live = false }: AgentStackProps) {
  if (agents.length === 0) return null;
  const shown = agents.slice(0, max);
  const extra = agents.length - shown.length;
  const overlap = -Math.round(size * 0.34);
  const extraStyle: CSSProperties = {
    width: size,
    height: size,
    borderRadius: "50%",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
    boxSizing: "border-box",
    marginLeft: overlap,
    background: "var(--ct-bg-active)",
    color: "color-mix(in srgb, var(--ct-fg) 80%, transparent)",
    border: "1.5px solid var(--ct-bg)",
    fontSize: Math.round(size * 0.38),
    fontWeight: 800,
    lineHeight: 1,
    fontFamily: "var(--ct-font-ui)",
    userSelect: "none",
  };
  return (
    <span data-testid="canvas-agent-stack" data-agent-count={agents.length} style={{ display: "inline-flex", alignItems: "center" }}>
      {shown.map((agent, i) => (
        <span key={agent.id} style={{ marginLeft: i === 0 ? 0 : overlap, zIndex: shown.length - i, position: "relative" }}>
          <AgentCircle agent={agent} size={size} live={live && i === 0} />
        </span>
      ))}
      {extra > 0 && (
        <span data-testid="canvas-agent-overflow" title={agents.slice(max).map((a) => a.label).join(", ")} style={extraStyle}>
          +{extra}
        </span>
      )}
    </span>
  );
}

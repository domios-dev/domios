import type { MissionAgent } from "./types";

interface AgentAvatarProps {
  agent: MissionAgent;
  showName?: boolean;
  onClick?: (tabId: string) => void;
}

export function AgentAvatar({ agent, showName, onClick }: AgentAvatarProps) {
  const initials = agent.name.slice(0, 2).toUpperCase();
  const liveClass = agent.state === "working" ? "ct-presence-live" : agent.state === "qa-fail" ? "mc-avatar-qa-fail" : undefined;
  const staleClass = agent.state === "idle" || agent.state === "waiting" ? "ct-presence-stale" : undefined;
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <div
        className={["mc-avatar", onClick && "mc-avatar-clickable", liveClass, staleClass].filter(Boolean).join(" ")}
        data-state={agent.state}
        data-provider={agent.provider}
        data-mc-interactive={onClick ? "true" : undefined}
        title={onClick ? `${agent.name} (${agent.provider}) — open tab` : `${agent.name} (${agent.provider})`}
        data-testid={`avatar-${agent.id}`}
        role={onClick ? "button" : undefined}
        tabIndex={onClick ? 0 : undefined}
        onClick={onClick ? (e) => { e.stopPropagation(); onClick(agent.id); } : undefined}
        onKeyDown={onClick ? (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onClick(agent.id); } } : undefined}
      >
        {initials}
      </div>
      {showName && (
        <div style={{ minWidth: 0 }}>
          <div style={{ fontSize: 10, fontWeight: 600 }}>{agent.name}</div>
          <div className="mc-detail-only" style={{ fontSize: 9, color: "var(--mc-muted)" }}>{agent.state}</div>
        </div>
      )}
    </div>
  );
}

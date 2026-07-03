import type { MissionAgent } from "./types";

interface AgentAvatarProps {
  agent: MissionAgent;
  showName?: boolean;
}

export function AgentAvatar({ agent, showName }: AgentAvatarProps) {
  const initials = agent.name.slice(0, 2).toUpperCase();
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <div
        className="mc-avatar"
        data-state={agent.state}
        data-provider={agent.provider}
        title={`${agent.name} (${agent.provider})`}
        data-testid={`avatar-${agent.id}`}
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

import { AgentAvatar } from "./AgentAvatar";
import type { MissionAgent } from "./types";

interface CrewStripProps {
  crew: MissionAgent[];
  onAgentClick?: (tabId: string) => void;
  /** Fallback text when nobody is working (e.g. "unclaimed"). */
  emptyLabel?: string;
}

const MAX_AVATARS = 4;

export function CrewStrip({ crew, onAgentClick, emptyLabel = "unclaimed" }: CrewStripProps) {
  if (crew.length === 0) {
    return <span className="mc-crew" data-testid="crew-strip">{emptyLabel}</span>;
  }
  const shown = crew.slice(0, MAX_AVATARS);
  const working = crew.filter((a) => a.state === "working").length;
  return (
    <span className="mc-crew" data-testid="crew-strip">
      {shown.map((agent) => (
        <AgentAvatar key={agent.id} agent={agent} onClick={onAgentClick} />
      ))}
      <span className="mc-crew-count">
        {crew.length > MAX_AVATARS ? `+${crew.length - MAX_AVATARS} · ` : ""}
        {working} working
      </span>
    </span>
  );
}

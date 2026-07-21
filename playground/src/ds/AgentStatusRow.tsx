import type { HTMLAttributes } from "react";
import { LivingDots } from "./LivingDots";
import "./agent-status-row.css";

export type AgentStatusPhase = "work" | "thinking" | "streaming" | "idle";

export interface AgentStatusRowProps extends HTMLAttributes<HTMLDivElement> {
  providerName: string;
  model: string;
  activity: string;
  detail: string;
  phase: AgentStatusPhase;
}

export function AgentStatusRow({ providerName, model, activity, detail, phase, className = "", ...props }: AgentStatusRowProps) {
  const line = [providerName, model, activity].filter(Boolean).join(" · ");
  return <div className={`ds-root ds-agent-status-row ${className}`} data-agent-phase={phase} role="status" aria-live="polite" aria-label={`Agent ${line} ${detail}`} {...props}>
    <LivingDots size="small" tone={phase === "thinking" ? "thinking" : "accent"} rhythm={phase === "streaming" ? "streaming" : phase === "thinking" ? "thinking" : "work"} idle={phase === "idle"} />
    <span className="ds-agent-status-row__message">{line} <span className="ds-agent-status-row__detail">({detail})</span></span>
  </div>;
}

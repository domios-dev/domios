import type { HTMLAttributes } from "react";
import { ChevronDown, Square } from "lucide-react";
import { LivingDots } from "./LivingDots";
import "./agent-status-row.css";

export type AgentStatusPhase = "work" | "thinking" | "streaming" | "idle";

export interface AgentStatusRowProps extends HTMLAttributes<HTMLDivElement> {
  providerName: string;
  glyph: string;
  glyphColor?: string;
  model: string;
  effort?: string;
  status: string;
  phase: AgentStatusPhase;
  switchable?: boolean;
}

export function AgentStatusRow({ providerName, glyph, glyphColor = "currentColor", model, effort, status, phase, switchable = true, className = "", ...props }: AgentStatusRowProps) {
  const working = phase !== "idle";
  const identity = [model, effort].filter(Boolean).join(" · ");
  const capsule = <>
    <span aria-hidden="true" className="ds-status-identity__glyph" title={providerName}><span className="ds-provider-glyph" style={{ color: glyphColor }}>{glyph}</span></span>
    <span className="ds-status-identity__text">{identity}</span>
  </>;
  return <div className={`ds-root ds-agent-row ds-agent-row--chat ${className}`} data-agent-state={working ? "working" : "idle"} data-agent-phase={working ? phase : "work"} role="status" aria-live="polite" aria-label={`Agent ${[providerName, identity, status].filter(Boolean).join(" · ")}`} {...props}>
    <span className="ds-agent-msg ds-agent-identity">
      <span className="ds-status-identity-wrap">
        {switchable
          ? <button type="button" className="ds-status-identity ds-status-identity--switchable" aria-haspopup="dialog" aria-label={`Change model, currently ${identity}`} title="Change model">
            {capsule}
            <ChevronDown size={12} strokeWidth={2.25} aria-hidden className="ds-status-identity__chevron" />
          </button>
          : <span className="ds-status-identity ds-status-identity--static" title={providerName}>{capsule}</span>}
      </span>
    </span>
    <LivingDots size="small" tone={working ? (phase === "thinking" ? "thinking" : "accent") : "muted"} rhythm={phase === "idle" ? "work" : phase} idle={!working} />
    <span className="ds-agent-msg ds-agent-row__status" title={status || undefined}>{status}</span>
    <button type="button" className={`ds-agent-stop${working ? "" : " ds-agent-stop--slot"}`} disabled={!working} aria-hidden={!working || undefined} tabIndex={working ? undefined : -1} aria-label="Stop the agent" title={working ? "Stop the agent" : undefined}>
      <Square size={9} strokeWidth={0} fill="currentColor" aria-hidden />
      Stop
    </button>
  </div>;
}

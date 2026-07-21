import type { ButtonHTMLAttributes, ReactNode } from "react";
import { LivingDots } from "./LivingDots";
import "./agent-tree-row.css";

export interface AgentTreeRowProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  name: string;
  role: string;
  model: string;
  status: "idle" | "working";
  selected?: boolean;
  trailing?: ReactNode;
}

export function AgentTreeRow({ name, role, model, status, selected = false, trailing, className = "", ...props }: AgentTreeRowProps) {
  return <button type="button" className={`ds-root ds-agent-tree-row${status === "working" ? " ds-agent-tree-row--working" : ""}${selected ? " ds-agent-tree-row--selected" : ""} ${className}`} aria-current={selected || undefined} {...props}>
    {status === "working" ? <LivingDots size="small" tone="muted" /> : <span className="ds-agent-tree-row__icon" aria-hidden>✦</span>}
    <span className="ds-agent-tree-row__copy"><span className="ds-agent-tree-row__name">{name}</span><span className="ds-agent-tree-row__role">{role}</span></span>
    <span className="ds-agent-tree-row__badge">{model}</span>
    {trailing}
  </button>;
}

import type { ButtonHTMLAttributes, CSSProperties } from "react";
import "./plan-entry-card.css";

export interface PlanEntryCardProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  title: string;
  subtitle: string;
  completed: number;
  total: number;
  selected?: boolean;
}

export function PlanEntryCard({ title, subtitle, completed, total, selected = false, className = "", ...props }: PlanEntryCardProps) {
  const progress = total ? Math.min(100, completed / total * 100) : 0;
  return <button type="button" className={`ds-root ds-plan-entry${selected ? " ds-plan-entry--selected" : ""} ${className}`} aria-current={selected || undefined} {...props}>
    <span key={completed} className="ds-plan-entry__donut" style={{ "--progress": `${progress}%` } as CSSProperties}><i /></span>
    <span className="ds-plan-entry__copy"><span>{title}</span><small>{subtitle}</small></span>
    <span key={`${completed}/${total}`} className="ds-plan-entry__count">{completed}/{total}</span>
  </button>;
}

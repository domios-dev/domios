import { useEffect, useRef, useState, type ButtonHTMLAttributes } from "react";
import { LivingDots } from "./LivingDots";
import { PlanStatusBreakdown, type PlanStatusCounts } from "./PlanStatusBreakdown";
import { SegmentedDonut } from "./SegmentedDonut";
import "./plan-entry-card.css";

export interface PlanEntryCardProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  title: string;
  subtitle: string;
  counts: PlanStatusCounts;
  selected?: boolean;
  working?: boolean;
}

export function PlanEntryCard({ title, subtitle, counts, selected = false, working = false, className = "", onAnimationEnd, ...props }: PlanEntryCardProps) {
  const total = counts.todo + counts.review + counts.blocked + counts.done;
  const wasComplete = useRef(counts.done > 0 && counts.done === total);
  const [allDone, setAllDone] = useState(false);
  useEffect(() => {
    const complete = counts.done > 0 && counts.done === total;
    if (complete && !wasComplete.current) setAllDone(true);
    wasComplete.current = complete;
  }, [counts.done, total]);
  const segments = (Object.entries(counts) as Array<[keyof PlanStatusCounts, number]>).map(([status, count]) => ({ status, count }));

  return <button type="button" className={`ds-root ds-plan-entry${selected ? " ds-plan-entry--selected" : ""}${allDone ? " is-all-done" : ""} ${className}`} aria-current={selected || undefined} onAnimationEnd={(event) => { if (event.animationName === "ds-plan-all-done-sweep") setAllDone(false); onAnimationEnd?.(event); }} {...props}>
    <SegmentedDonut key={JSON.stringify(counts)} className="ds-plan-entry__donut" segments={segments} size={22} aria-label={`Task status composition: ${counts.todo} todo, ${counts.review} review, ${counts.blocked} blocked, ${counts.done} done`} />
    <span className="ds-plan-entry__copy">
      <span className="ds-plan-entry__title"><span className="ds-plan-entry__title-text">{title}</span>{working && <LivingDots size="small" tone="muted" />}</span>
      <small>{subtitle}</small>
      <PlanStatusBreakdown counts={counts} />
    </span>
    <span key={`${counts.done}/${total}`} className="ds-plan-entry__count">{counts.done}/{total}</span>
  </button>;
}

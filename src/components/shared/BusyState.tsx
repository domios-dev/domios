// Universal waiting grammar. ONE canon: the three-dot phase motion from
// agent-state.css (.ct-phase-dots), reused by every awaitable surface so a
// pane boot, a chat pending turn, a plan load, and a mem/mesh fetch all read
// the same. Reduced-motion + static fallback live in agent-state.css.
//
// This file is the ONLY place bespoke spinner / loading strings may live —
// the grep gate (BusyState.grepgate.test.ts) enforces it. New waiting surface
// → render <BusyState>, never a fresh animate-spin or "Loading..." literal.

import React from "react";
import { type BusyLabelKey, busyLabel } from "./busyStateLabels";

type BusyPhase = "work" | "thinking" | "streaming";

export interface BusyStateProps {
  labelKey?: BusyLabelKey;
  label?: string;
  phase?: BusyPhase;
  inline?: boolean;
  hideLabel?: boolean;
  className?: string;
  style?: React.CSSProperties;
  locale?: string;
  "data-testid"?: string;
}

export function BusyState({
  labelKey = "generic",
  label,
  phase = "thinking",
  inline = false,
  hideLabel = false,
  className,
  style,
  locale = "en",
  "data-testid": testId = "busy-state",
}: BusyStateProps) {
  const text = label ?? busyLabel(labelKey, locale);
  const base = inline ? "inline-flex items-baseline gap-2" : "ct-agent-row";
  return (
    <span
      className={`${base} ${className ?? ""}`.trim()}
      data-agent-state="working"
      data-agent-phase={phase}
      data-testid={testId}
      role="status"
      aria-live="polite"
      aria-label={text}
      style={style}
    >
      <span className="ct-phase-dots" aria-hidden="true">
        <i />
        <i />
        <i />
      </span>
      {!hideLabel && text ? <span className="ct-agent-msg">{text}</span> : null}
    </span>
  );
}

export interface BusySpinnerProps {
  spinning?: boolean;
  size?: number;
  className?: string;
  style?: React.CSSProperties;
  icon?: React.ComponentType<{ size?: number; className?: string; style?: React.CSSProperties }>;
  "data-testid"?: string;
}

// Icon-affordance form of the busy grammar (refresh buttons, inline pending
// icons). Single owner of the animate-spin class; reduced-motion neutralized
// via .ct-busy-spinner in agent-state.css.
export function BusySpinner({
  spinning = true,
  size = 14,
  className,
  style,
  icon: Icon,
  "data-testid": testId,
}: BusySpinnerProps) {
  const spinClass = spinning ? "animate-spin ct-busy-spinner" : "";
  const cls = `${spinClass} ${className ?? ""}`.trim();
  if (Icon) {
    return <Icon size={size} className={cls} style={style} />;
  }
  return (
    <span
      className={`ct-busy-ring ${spinning ? "animate-spin ct-busy-spinner" : ""} ${className ?? ""}`.trim()}
      style={{ width: size, height: size, ...style }}
      data-testid={testId}
      role="status"
      aria-label="Loading"
    />
  );
}

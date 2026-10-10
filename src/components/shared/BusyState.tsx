import { TabLoadingIndicator } from "./TabLoadingIndicator";
import type { TabActivity } from "../../lib/tabActivity";

import React from "react";
import { ActivityDots, type ActivityDotsSize } from "./ActivityDots";
import { type BusyLabelKey, busyLabel } from "./busyStateLabels";

type BusyPhase = "work" | "thinking" | "streaming";

export interface BusyStateProps {
  activity?: TabActivity;
  labelKey?: BusyLabelKey;
  label?: string;
  phase?: BusyPhase;
  inline?: boolean;
  dotSize?: ActivityDotsSize;
  hideLabel?: boolean;
  className?: string;
  style?: React.CSSProperties;
  locale?: string;
  "data-testid"?: string;
}

export function BusyState({
  activity,
  labelKey = "generic",
  label,
  phase = "thinking",
  inline = false,
  dotSize = inline ? "small" : "medium",
  hideLabel = false,
  className,
  style,
  locale = "en",
  "data-testid": testId = "busy-state",
}: BusyStateProps) {
  const text = activity?.label ?? label ?? busyLabel(labelKey, locale);
  if (activity?.kind === "starting") return <TabLoadingIndicator label={text} />;
  const base = inline ? "inline-flex items-baseline gap-2" : "ct-agent-row";
  return (
    <span
      className={`${base} ${className ?? ""}`.trim()}
      data-agent-state={activity?.state ?? "working"}
      data-agent-phase={phase}
      data-testid={testId}
      role="status"
      aria-live="polite"
      aria-label={text}
      style={style}
    >
      <ActivityDots
        size={dotSize}
        tone={phase === "thinking" ? "thinking" : "accent"}
        rhythm={phase}
        active={activity?.active ?? true}
      />
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

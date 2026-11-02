import { TabLoadingIndicator } from "./TabLoadingIndicator";
import type { TabActivity } from "../../lib/tabActivity";

import React from "react";
import { ActivityDots, type ActivityDotsSize } from "./ActivityDots";
import { BrandMark } from "../brand/BrandMark";
import { type BusyLabelKey, busyLabel } from "./busyStateLabels";

type BusyPhase = "work" | "thinking" | "streaming";

export interface BusyStateProps {
  activity?: TabActivity;
  labelKey?: BusyLabelKey;
  label?: string;
  phase?: BusyPhase;
  inline?: boolean;
  indicator?: "dots" | "brand";
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
  indicator = "dots",
  dotSize = inline ? "small" : "medium",
  hideLabel = false,
  className,
  style,
  locale = "en",
  "data-testid": testId = "busy-state",
}: BusyStateProps) {
  const text = activity?.label ?? label ?? busyLabel(labelKey, locale);
  if (activity?.kind === "starting") return <TabLoadingIndicator label={text} />;
  const base = inline ? `inline-flex ${indicator === "brand" ? "items-center" : "items-baseline"} gap-2` : "ct-agent-row";
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
      {indicator === "brand" ? <span className="ct-busy-brand" aria-hidden="true" data-active={activity?.active ?? true}>
        <BrandMark size={16} />
      </span> : <ActivityDots
        size={dotSize}
        tone={phase === "thinking" ? "thinking" : "accent"}
        rhythm={phase}
        active={activity?.active ?? true}
      />}
      {!hideLabel && text ? <span className="ct-agent-msg">{text}</span> : null}
    </span>
  );
}

export interface BusySpinnerProps {
  spinning?: boolean;
  size?: number;
  className?: string;
  style?: React.CSSProperties;
  icon: React.ComponentType<{ size?: number; className?: string; style?: React.CSSProperties }>;
}

/** Spins a control's own icon while its action runs, such as Refresh. Anything else that waits shows the dots
 *  (BusyState, a Button's `loading`) or, for a whole area, Loader; never a separate spinner. */
export function BusySpinner({ spinning = true, size = 14, className, style, icon: Icon }: BusySpinnerProps) {
  return <Icon size={size} className={`${spinning ? "animate-spin ct-busy-spinner" : ""} ${className ?? ""}`.trim()} style={style} />;
}

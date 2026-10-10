import { useEffect, useState } from "react";
import { ActivityDots, type ActivityDotsProps } from "../../../../src/components/shared/ActivityDots";
import { defineSpecimen } from "../types";

function DotsSpecimen({ cooling = false, onAccent = false, ...props }: ActivityDotsProps & { cooling?: boolean; onAccent?: boolean }) {
  const [active, setActive] = useState(props.active ?? true);
  useEffect(() => {
    if (!cooling) return;
    const timer = window.setInterval(() => setActive((value) => !value), 2400);
    return () => window.clearInterval(timer);
  }, [cooling]);
  const dots = <ActivityDots {...props} active={cooling ? active : props.active} />;
  return onAccent ? <span className="gallery-row" style={{ padding: "6px 10px", borderRadius: 999, background: "var(--ct-accent)" }}>{dots}</span> : dots;
}

export default defineSpecimen({
  id: "activity-dots",
  title: "Activity dots",
  category: "Status",
  where: "src/components/shared/ActivityDots.tsx",
  notes: "The shared squash-jump signature for every live and idle agent state. Inactive dots stay in place, still and muted; the cooling stage toggles activity to show the cooldown and the reactivation.",
  Component: DotsSpecimen,
  states: [
    { id: "accent-medium", label: "Accent · medium · work", props: { tone: "accent", size: "medium" } },
    { id: "thinking-small", label: "Thinking · small", props: { tone: "thinking", size: "small", rhythm: "thinking" } },
    { id: "streaming-small", label: "Streaming · small", props: { tone: "accent", size: "small", rhythm: "streaming" } },
    { id: "muted-small", label: "Muted · small (sidebar)", props: { tone: "muted", size: "small" } },
    { id: "muted-micro", label: "Muted · micro", props: { tone: "muted", size: "micro" } },
    { id: "workspace-badge", label: "Workspace badge", props: { tone: "muted", size: "micro", variant: "workspace-badge" } },
    { id: "on-accent", label: "On accent", props: { tone: "on-accent", size: "small", onAccent: true } },
    { id: "still", label: "Inactive · still", props: { tone: "muted", size: "small", active: false } },
    { id: "cooling", label: "Cooling and reactivating (loops)", props: { tone: "accent", size: "small", cooling: true } },
  ],
});

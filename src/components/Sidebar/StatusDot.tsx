import type { PaneStatus } from "../../lib/types";

const STATUS_COLORS: Record<PaneStatus, string> = {
  Working: "var(--ct-green, #7ee787)",
  Waiting: "var(--ct-yellow, #d29922)",
  Idle: "var(--ct-muted, #484f58)",
  Dead: "var(--ct-red, #f85149)",
};

export function StatusDot({ status, animate }: { status: PaneStatus | null; animate?: boolean }) {
  if (!status || status === "Idle") return null;
  const color = STATUS_COLORS[status];
  return (
    <span
      className={`inline-block w-2 h-2 rounded-full shrink-0 ${animate && status === "Working" ? "animate-pulse" : ""}`}
      style={{ backgroundColor: color }}
      title={status}
    />
  );
}

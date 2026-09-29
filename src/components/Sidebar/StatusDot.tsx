import type { TabStatus } from "../../lib/types";

const STATUS_COLORS: Record<TabStatus, string> = {
  Working: "var(--ct-green, #7ee787)",
  Waiting: "var(--ct-yellow, #d29922)",
  Idle: "var(--ct-muted, #484f58)",
  Error: "var(--ct-red, #f85149)",
  Dead: "var(--ct-red, #f85149)",
};

export function StatusDot({
  status,
  animate,
  label = status ?? undefined,
}: {
  status: TabStatus | null;
  animate?: boolean;
  label?: string;
}) {
  if (!status || status === "Idle") return null;
  const color = STATUS_COLORS[status];
  return (
    <span
      role="img"
      aria-label={label}
      className={`inline-block w-2 h-2 rounded-full shrink-0 ${animate && status === "Working" ? "animate-pulse" : ""}`}
      style={{ backgroundColor: color }}
      title={label}
    />
  );
}

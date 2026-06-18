import { Loader2 } from "lucide-react";
import type { PluginActivity } from "../../lib/types";
import type { PluginActivityDisplay } from "../../lib/usePluginActivity";

export function PluginActivityInline({
  activity,
  display,
  compact = false,
}: {
  activity: PluginActivity | undefined;
  display: PluginActivityDisplay;
  compact?: boolean;
}) {
  if (!activity || activity.status !== "running") return null;

  const percent =
    typeof activity.percent === "number"
      ? Math.max(0, Math.min(100, activity.percent))
      : null;

  return (
    <div
      role="status"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 5,
        marginTop: compact ? 4 : 10,
        minWidth: 0,
      }}
    >
      <div style={{ display: "flex", alignItems: "flex-start", gap: 6, minWidth: 0 }}>
        <Loader2
          size={compact ? 12 : 14}
          className="animate-spin"
          style={{ color: "var(--ct-accent, #818cf8)", flex: "none", marginTop: compact ? 1 : 2 }}
        />
        <div style={{ minWidth: 0, flex: 1 }}>
          <div
            style={{
              color: "var(--ct-fg)",
              fontSize: compact ? 10.5 : 12,
              fontWeight: 560,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {display.displayName}
          </div>
          <div
            style={{
              color: "var(--ct-muted)",
              fontSize: compact ? 10.5 : 12,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              marginTop: compact ? 0 : 1,
            }}
          >
            {activity.label}
          </div>
        </div>
      </div>
      {percent !== null && (
        <div
          aria-label={`${Math.round(percent)}%`}
          style={{
            height: compact ? 3 : 4,
            borderRadius: 999,
            background: "var(--ct-bg-elev, rgba(255,255,255,0.08))",
            overflow: "hidden",
          }}
        >
          <div
            style={{
              width: `${percent}%`,
              height: "100%",
              background: "var(--ct-accent, #818cf8)",
            }}
          />
        </div>
      )}
    </div>
  );
}

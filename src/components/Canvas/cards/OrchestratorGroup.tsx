import { memo } from "react";
import { Boxes } from "lucide-react";
import type { OwnerGroup } from "../layout";

export interface OrchestratorGroupProps {
  group: OwnerGroup;
  onOpen?: (paneId: string) => void;
}

// The orchestrator is a FREE-FLOATING entity — like a user parked in space near the
// plans it runs — linked to each plan by a thin connector (drawn by CanvasView). NOT
// a rectangle wrapping the plans. Click → focus its pane. Unowned → claimable.
// Position is read from group.box (layout-stable) so memo holds across pan/zoom.
export const OrchestratorGroup = memo(function OrchestratorGroup({ group, onOpen }: OrchestratorGroupProps) {
  const claimable = !group.ownerPaneId;
  const planCount = group.planIds.length;
  const accent = claimable ? "var(--ct-border-default)" : "rgba(56, 189, 248, 0.95)";

  return (
    <button
      type="button"
      data-testid="canvas-orchestrator-group"
      data-owner-key={group.ownerKey}
      data-claimable={claimable ? "true" : "false"}
      disabled={claimable}
      onClick={() => group.ownerPaneId && onOpen?.(group.ownerPaneId)}
      onMouseDown={(e) => e.stopPropagation()}
      title={claimable ? "No orchestrator — claimable" : `Open ${group.title}`}
      style={{
        left: group.box.x,
        top: group.box.y,
        position: "absolute",
        display: "inline-flex",
        alignItems: "center",
        gap: 8,
        padding: "5px 12px 5px 6px",
        borderRadius: 999,
        border: `1px solid ${claimable ? "var(--ct-border-subtle)" : "color-mix(in srgb, var(--ct-border-focus) 45%, transparent)"}`,
        background: "var(--ct-bg)",
        color: "var(--ct-fg)",
        fontFamily: "var(--ct-font-ui)",
        fontSize: "var(--ct-font-size-sm)",
        fontWeight: 700,
        letterSpacing: "-0.005em",
        whiteSpace: "nowrap",
        cursor: claimable ? "default" : "pointer",
        boxShadow: "var(--ct-shadow-md)",
        pointerEvents: "auto",
      }}
    >
      <span
        data-testid="canvas-orchestrator-avatar"
        aria-hidden
        style={{
          width: 22,
          height: 22,
          borderRadius: "50%",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
          background: claimable ? "var(--ct-bg-active)" : "color-mix(in srgb, rgba(56,189,248,1) 20%, var(--ct-bg))",
          border: `1px solid ${accent}`,
        }}
      >
        <Boxes size={12} style={{ opacity: 0.85, color: claimable ? undefined : "var(--ct-border-focus)" }} />
      </span>
      <span data-testid="canvas-orchestrator-label" style={{ maxWidth: 200, overflow: "hidden", textOverflow: "ellipsis" }}>
        {group.title}
      </span>
      <span
        data-testid="canvas-orchestrator-plan-count"
        style={{
          fontSize: "0.72em",
          fontWeight: 700,
          fontFamily: "var(--ct-font-mono)",
          padding: "0 6px",
          borderRadius: 999,
          background: "var(--ct-bg-active)",
          border: "1px solid var(--ct-border-subtle)",
          opacity: 0.85,
        }}
      >
        {planCount} {planCount === 1 ? "plan" : "plans"}
      </span>
      <span
        aria-hidden
        style={{
          width: 7,
          height: 7,
          borderRadius: "50%",
          background: accent,
          boxShadow: claimable ? "none" : "0 0 0 3px color-mix(in srgb, rgba(56,189,248,1) 22%, transparent)",
        }}
      />
    </button>
  );
});

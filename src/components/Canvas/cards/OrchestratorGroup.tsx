import type { CSSProperties } from "react";
import { Boxes, Users } from "lucide-react";
import type { OwnerGroup } from "../layout";

export interface OrchestratorGroupProps {
  group: OwnerGroup;
  style: CSSProperties;
  scale: number;
  onOpen?: (paneId: string) => void;
}

// Soft labeled wrapper around all plans owned by one orchestrator (§2.5). The
// label is clickable → focuses that orchestrator's pane. Unowned → claimable.
// Group LOD is camera-driven (a group has no single footprint): far out it stays a
// meaningful pill — owner + plan count; once readable it opens the structural home
// for the live agent-presence cluster (north-star #2, populated a later iter).
const GLANCE_SCALE = 0.5;

export function OrchestratorGroup({ group, style, scale, onOpen }: OrchestratorGroupProps) {
  const claimable = !group.ownerPaneId;
  const planCount = group.planIds.length;
  const readable = scale >= GLANCE_SCALE;
  const accent = claimable ? "var(--ct-border-default)" : "rgba(56, 189, 248, 0.95)";

  return (
    <div
      data-testid="canvas-orchestrator-group"
      data-owner-key={group.ownerKey}
      data-claimable={claimable ? "true" : "false"}
      style={{
        ...style,
        position: "absolute",
        boxSizing: "border-box",
        border: `1px ${claimable ? "dashed" : "solid"} ${claimable ? "var(--ct-border-subtle)" : "color-mix(in srgb, var(--ct-border-focus) 30%, var(--ct-border-subtle))"}`,
        borderRadius: "var(--ct-radius-lg)",
        background: claimable
          ? "color-mix(in srgb, var(--ct-fg) 2%, transparent)"
          : "color-mix(in srgb, var(--ct-border-focus) 4.5%, transparent)",
        pointerEvents: "none",
      }}
    >
      <button
        type="button"
        data-testid="canvas-orchestrator-label"
        disabled={claimable}
        onClick={() => group.ownerPaneId && onOpen?.(group.ownerPaneId)}
        style={{
          position: "absolute",
          top: 7,
          left: 12,
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "4px 12px",
          borderRadius: 999,
          border: `1px solid ${claimable ? "var(--ct-border-subtle)" : "color-mix(in srgb, var(--ct-border-focus) 45%, transparent)"}`,
          background: "var(--ct-bg)",
          color: "var(--ct-fg)",
          fontFamily: "var(--ct-font-ui)",
          fontSize: "var(--ct-font-size-sm)",
          fontWeight: 700,
          letterSpacing: "-0.005em",
          pointerEvents: "auto",
          cursor: claimable ? "default" : "pointer",
          boxShadow: "var(--ct-shadow-md)",
        }}
        title={claimable ? "No orchestrator — claimable" : `Open ${group.title}`}
      >
        <Boxes size={14} style={{ opacity: 0.8, color: claimable ? undefined : "var(--ct-border-focus)" }} />
        <span style={{ maxWidth: 220, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
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
          style={{
            width: 7,
            height: 7,
            borderRadius: "50%",
            background: accent,
            boxShadow: claimable ? "none" : "0 0 0 3px color-mix(in srgb, rgba(56,189,248,1) 22%, transparent)",
          }}
        />
      </button>

      {/* §emergence — structural home for the live agent-presence cluster. Reserved
          slot only (north-star #2): the swarm of labeled agent circles mounts here
          once presence data is wired. Render the frame solely when readable. */}
      {readable && !claimable && (
        <div
          data-testid="canvas-orchestrator-presence-slot"
          data-presence-state="reserved"
          style={{
            position: "absolute",
            top: 7,
            right: 10,
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "3px 9px",
            borderRadius: 999,
            border: "1px dashed color-mix(in srgb, var(--ct-border-focus) 35%, var(--ct-border-subtle))",
            background: "color-mix(in srgb, var(--ct-border-focus) 5%, var(--ct-bg))",
            color: "var(--ct-fg)",
            fontFamily: "var(--ct-font-ui)",
            fontSize: "0.66em",
            fontWeight: 700,
            letterSpacing: "0.04em",
            textTransform: "uppercase",
            opacity: 0.55,
            pointerEvents: "none",
          }}
        >
          <Users size={11} style={{ opacity: 0.7 }} />
          <span>Swarm</span>
        </div>
      )}
    </div>
  );
}

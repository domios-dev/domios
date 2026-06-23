import type { CSSProperties } from "react";
import { Boxes } from "lucide-react";
import type { OwnerGroup } from "../layout";

export interface OrchestratorGroupProps {
  group: OwnerGroup;
  style: CSSProperties;
  onOpen?: (paneId: string) => void;
}

// Soft labeled wrapper around all plans owned by one orchestrator (§2.5). The
// label is clickable → focuses that orchestrator's pane. Unowned → claimable.
export function OrchestratorGroup({ group, style, onOpen }: OrchestratorGroupProps) {
  const claimable = !group.ownerPaneId;
  return (
    <div
      data-testid="canvas-orchestrator-group"
      data-owner-key={group.ownerKey}
      style={{
        ...style,
        position: "absolute",
        boxSizing: "border-box",
        border: `1.5px ${claimable ? "dashed" : "solid"} var(--ct-border-subtle)`,
        borderRadius: "var(--ct-radius-lg)",
        background: claimable
          ? "color-mix(in srgb, var(--ct-fg) 2%, transparent)"
          : "color-mix(in srgb, var(--ct-border-focus) 5%, transparent)",
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
          top: 8,
          left: 14,
          display: "flex",
          alignItems: "center",
          gap: 7,
          padding: "3px 10px",
          borderRadius: 999,
          border: "1px solid var(--ct-border-subtle)",
          background: "var(--ct-bg)",
          color: "var(--ct-fg)",
          fontFamily: "var(--ct-font-ui)",
          fontSize: "var(--ct-font-size-sm)",
          fontWeight: 600,
          pointerEvents: "auto",
          cursor: claimable ? "default" : "pointer",
          boxShadow: "var(--ct-shadow-sm)",
        }}
        title={claimable ? "No orchestrator — claimable" : `Open ${group.title}`}
      >
        <Boxes size={13} style={{ opacity: 0.75 }} />
        <span style={{ maxWidth: 220, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {group.title}
        </span>
        <span
          style={{
            width: 7,
            height: 7,
            borderRadius: "50%",
            background: claimable ? "var(--ct-border-default)" : "rgba(56, 189, 248, 0.95)",
            boxShadow: claimable ? "none" : "0 0 0 3px color-mix(in srgb, rgba(56,189,248,1) 22%, transparent)",
          }}
        />
      </button>
    </div>
  );
}

import { useState, type CSSProperties } from "react";
import { ChevronDown, ChevronUp, GitBranch } from "lucide-react";
import { AgentCircle } from "./presence/AgentCircle";
import type { SwarmMember, SwarmRoster } from "./swarmRoster";

// EMERGENCE (north-star #2): viewport-anchored swarm roster. Collapsed = a quiet
// count chip + expand control; faces and labels live only in the opened roster.

const FACE_CAP = 5;

export interface CanvasSwarmClusterProps {
  roster: SwarmRoster;
  onFlyToTask: (taskId: string) => void;
  /** Push below the focus-mode close button when it occupies the top-right. */
  reserveTopRight?: boolean;
}

const shell: CSSProperties = {
  position: "absolute",
  right: 12,
  zIndex: 22,
  display: "flex",
  flexDirection: "column",
  alignItems: "flex-end",
  gap: 6,
  fontFamily: "var(--ct-font-ui)",
  maxWidth: 320,
};

const surface: CSSProperties = {
  background: "color-mix(in srgb, var(--ct-titlebar-bg) 92%, transparent)",
  border: "1px solid var(--ct-border-default)",
  boxShadow: "0 8px 24px -10px rgba(0,0,0,0.55), 0 2px 6px -3px rgba(0,0,0,0.4)",
  backdropFilter: "blur(8px)",
};

function memberTitle(m: SwarmMember): string {
  const parts = [m.agent.label];
  if (m.agent.role) parts.push(`· ${m.agent.role}`);
  if (m.agent.isPeer) parts.push("(peer)");
  parts.push(`— ${m.live ? "working on" : "assigned"} "${m.taskTitle}"`);
  if (m.taskCount > 1) parts.push(`(+${m.taskCount - 1} more task${m.taskCount > 2 ? "s" : ""})`);
  if (m.branch) parts.push(`on ${m.branch}`);
  return parts.join(" ");
}

function FaceButton({ member, onFly }: { member: SwarmMember; onFly: () => void }) {
  return (
    <button
      type="button"
      data-testid="canvas-swarm-face"
      data-agent-id={member.agent.id}
      data-task-id={member.taskId}
      data-live={member.live ? "true" : "false"}
      onClick={(e) => {
        e.stopPropagation();
        onFly();
      }}
      title={memberTitle(member)}
      aria-label={memberTitle(member)}
      style={{
        // Negative inset = the overlapping multiplayer stack idiom (mirrors AgentStack).
        marginLeft: -6,
        padding: 0,
        border: "none",
        background: "transparent",
        cursor: "pointer",
        lineHeight: 0,
        borderRadius: "50%",
        transition: "transform 120ms var(--ct-ease-decel, ease)",
      }}
      onMouseEnter={(e) => (e.currentTarget.style.transform = "translateY(-2px)")}
      onMouseLeave={(e) => (e.currentTarget.style.transform = "translateY(0)")}
    >
      <AgentCircle agent={member.agent} size={22} live={member.live} />
    </button>
  );
}

function RosterRow({ member, onFly }: { member: SwarmMember; onFly: () => void }) {
  return (
    <button
      type="button"
      data-testid="canvas-swarm-member"
      data-agent-id={member.agent.id}
      data-task-id={member.taskId}
      data-live={member.live ? "true" : "false"}
      onClick={(e) => {
        e.stopPropagation();
        onFly();
      }}
      title={memberTitle(member)}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 9,
        width: "100%",
        padding: "6px 10px",
        border: "none",
        borderRadius: "var(--ct-radius-sm)",
        background: "transparent",
        color: "var(--ct-fg)",
        cursor: "pointer",
        textAlign: "left",
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = "var(--ct-bg-hover, color-mix(in srgb, var(--ct-fg) 8%, transparent))")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <AgentCircle agent={member.agent} size={22} live={member.live} />
      <span style={{ display: "flex", flexDirection: "column", gap: 1, minWidth: 0, flex: 1 }}>
        <span style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
          <span style={{ fontSize: 12, fontWeight: 700, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {member.agent.label}
          </span>
          {member.agent.role && (
            <span style={{ fontSize: 10, fontWeight: 600, opacity: 0.6, flexShrink: 0 }}>
              {member.agent.isPeer ? "peer" : member.agent.role}
            </span>
          )}
        </span>
        <span style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0, fontSize: 11, opacity: 0.72 }}>
          <span style={{ flexShrink: 0, opacity: 0.7 }}>→</span>
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{member.taskTitle}</span>
        </span>
      </span>
      {member.branch && (
        <span
          title={member.branch}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 3,
            flexShrink: 0,
            maxWidth: 92,
            padding: "1px 6px",
            borderRadius: 999,
            fontSize: 10,
            fontWeight: 600,
            fontVariantNumeric: "tabular-nums",
            color: "color-mix(in srgb, var(--ct-fg) 78%, transparent)",
            background: "color-mix(in srgb, var(--ct-fg) 7%, transparent)",
            border: "1px solid var(--ct-border-subtle)",
          }}
        >
          <GitBranch size={9} style={{ flexShrink: 0, opacity: 0.7 }} />
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{member.branch}</span>
        </span>
      )}
    </button>
  );
}

export function CanvasSwarmCluster({ roster, onFlyToTask, reserveTopRight }: CanvasSwarmClusterProps) {
  const [expanded, setExpanded] = useState(false);
  if (roster.members.length === 0) return null;

  const faces = roster.members.slice(0, FACE_CAP);
  const facesOverflow = roster.totalCount - faces.length;
  const Chevron = expanded ? ChevronUp : ChevronDown;
  const headline = roster.liveCount > 0 ? `${roster.liveCount} working` : `${roster.totalCount} idle`;

  return (
    <div
      data-testid="canvas-swarm-cluster"
      data-swarm-count={roster.totalCount}
      data-swarm-live={roster.liveCount}
      data-swarm-expanded={expanded ? "true" : "false"}
      style={{ ...shell, top: reserveTopRight ? 48 : 12 }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div
        style={{
          ...surface,
          display: "flex",
          alignItems: "center",
          gap: 8,
          height: 32,
          padding: "0 10px 0 12px",
          borderRadius: 999,
        }}
      >
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
          <span
            style={{
              width: 6,
              height: 6,
              borderRadius: "50%",
              background: roster.liveCount > 0 ? "var(--ct-green)" : "color-mix(in srgb, var(--ct-fg) 35%, transparent)",
            }}
          />
          <span style={{ fontSize: 11, fontWeight: 700, color: "var(--ct-fg)", whiteSpace: "nowrap", fontVariantNumeric: "tabular-nums" }}>
            {headline}
          </span>
          {!expanded && roster.totalCount > 0 && (
            <span style={{ fontSize: 10, fontWeight: 600, opacity: 0.55, fontVariantNumeric: "tabular-nums" }}>
              · {roster.totalCount}
            </span>
          )}
        </span>

        <button
          type="button"
          data-testid="canvas-swarm-expand"
          aria-expanded={expanded}
          aria-label={expanded ? "Collapse swarm roster" : "Expand swarm roster"}
          title={expanded ? "Collapse swarm roster" : "Expand swarm roster"}
          onClick={(e) => {
            e.stopPropagation();
            setExpanded((v) => !v);
          }}
          style={{
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            width: 22,
            height: 22,
            marginLeft: 2,
            borderRadius: "50%",
            border: "none",
            background: "transparent",
            color: "var(--ct-fg)",
            cursor: "pointer",
          }}
        >
          <Chevron size={14} />
        </button>
      </div>

      {expanded && (
        <div
          data-testid="canvas-swarm-roster"
          style={{
            ...surface,
            width: 300,
            maxWidth: "min(300px, 78vw)",
            borderRadius: "var(--ct-radius-md, 10px)",
            padding: 5,
            display: "flex",
            flexDirection: "column",
            maxHeight: "min(360px, 60vh)",
            overflowY: "auto",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "4px 8px 8px" }}>
            <span style={{ display: "inline-flex", alignItems: "center", gap: 6, paddingLeft: 2 }}>
              {faces.map((m) => (
                <FaceButton key={m.agent.id} member={m} onFly={() => onFlyToTask(m.taskId)} />
              ))}
              {facesOverflow > 0 && (
                <span
                  data-testid="canvas-swarm-faces-overflow"
                  title={`${facesOverflow} more`}
                  style={{
                    marginLeft: -6,
                    width: 22,
                    height: 22,
                    borderRadius: "50%",
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    background: "var(--ct-bg-active)",
                    color: "color-mix(in srgb, var(--ct-fg) 80%, transparent)",
                    border: "1.5px solid var(--ct-bg)",
                    fontSize: 9.5,
                    fontWeight: 800,
                    lineHeight: 1,
                  }}
                >
                  +{facesOverflow}
                </span>
              )}
            </span>
            <span style={{ fontSize: 10.5, fontWeight: 600, opacity: 0.55, fontVariantNumeric: "tabular-nums" }}>
              {roster.liveCount} working · {roster.totalCount} total
            </span>
          </div>
          {roster.members.map((m) => (
            <RosterRow key={m.agent.id} member={m} onFly={() => onFlyToTask(m.taskId)} />
          ))}
          {roster.overflow > 0 && (
            <span
              data-testid="canvas-swarm-overflow"
              style={{ padding: "6px 10px 4px", fontSize: 10.5, fontWeight: 600, opacity: 0.5 }}
            >
              +{roster.overflow} more agent{roster.overflow > 1 ? "s" : ""}
            </span>
          )}
        </div>
      )}
    </div>
  );
}

import type { CSSProperties } from "react";
import { FileText } from "lucide-react";
import { TASK_STATUS_COLORS } from "../settings";
import { sectionRows } from "../sectionRows";
import type { LaidOutNode } from "../layout";
import type { DriftItem, Plan, TagFacet, Task } from "../types";
import { resolveTags } from "../tagModel";
import type { World } from "../worlds";
import { planCoverageFraction } from "./coverage";

const frameBase: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow: "var(--ct-shadow-sm)",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
  pointerEvents: "auto",
};

function statusLabel(status: Task["status"]): string {
  return status.replace(/_/g, " ");
}

function StatusChip({ color, label }: { color: string; label: string }) {
  return (
    <span
      data-testid="canvas-task-status"
      style={{
        fontSize: "0.74em",
        padding: "0 0.5em",
        lineHeight: 1.7,
        borderRadius: 999,
        background: color,
        border: `1px solid color-mix(in srgb, ${color} 48%, transparent)`,
        color: "var(--ct-bg)",
        textTransform: "capitalize",
        fontWeight: 700,
        whiteSpace: "nowrap",
        flexShrink: 0,
      }}
    >
      {label}
    </span>
  );
}

function glanceFont(style: CSSProperties): number {
  const h = typeof style.height === "number" ? style.height : 140;
  return Math.max(30, Math.round(h * 0.32));
}

// W0 far-glance: a single big centered kind word ("Task"/"Plan"/"Spec") so a node reads at a
// glance when zoomed all the way out; plan/spec add a row of declared tags underneath. Detail
// (titles, status, coverage) only appears from W1 in — far out it would be unreadable noise.
function BigKindGlance({
  shared,
  label,
  accent,
  tags,
  extra,
}: {
  shared: Record<string, unknown> & { style: CSSProperties };
  label: string;
  accent?: string;
  tags?: string[];
  extra?: Record<string, unknown>;
}) {
  const font = glanceFont(shared.style);
  return (
    <div
      {...shared}
      {...extra}
      style={{
        ...shared.style,
        alignItems: "center",
        justifyContent: "center",
        gap: Math.round(font * 0.28),
        ...(accent ? { borderLeftWidth: 4, borderLeftStyle: "solid", borderLeftColor: accent } : null),
      }}
    >
      <span
        className="ct-canvas-text"
        style={{ fontSize: font, fontWeight: 800, letterSpacing: "-0.01em", lineHeight: 1, opacity: 0.92 }}
      >
        {label}
      </span>
      {tags && tags.length > 0 && (
        <div style={{ display: "flex", flexWrap: "wrap", justifyContent: "center", gap: Math.round(font * 0.12), maxWidth: "92%" }}>
          {tags.slice(0, 4).map((t) => (
            <span
              key={t}
              style={{
                fontSize: Math.round(font * 0.3),
                fontWeight: 600,
                padding: `${Math.round(font * 0.05)}px ${Math.round(font * 0.18)}px`,
                borderRadius: 999,
                background: "var(--ct-bg-active)",
                color: "var(--ct-fg-subtle)",
                whiteSpace: "nowrap",
              }}
            >
              {t}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

type PlanModel = Plan & { driftCount?: number };
type TaskModel = {
  task: Task;
  tagFacets?: TagFacet[];
  drift?: DriftItem;
};

export interface LightweightWorldCardProps {
  kind: "plan" | "task" | "spec";
  node: LaidOutNode;
  model: unknown;
  style: CSSProperties;
  world: World;
  onHover?: (taskId: string | null) => void;
  tagFilterDimmed?: boolean;
  tagFilterHighlighted?: boolean;
}

export function LightweightWorldCard({
  kind,
  node,
  model,
  style,
  world,
  onHover,
  tagFilterDimmed = false,
  tagFilterHighlighted = false,
}: LightweightWorldCardProps) {
  const semanticTestId = kind === "task" ? "canvas-task-card" : kind === "plan" ? "canvas-plan-node" : "canvas-spec-node";
  const shared = {
    "data-testid": "canvas-world-lightweight",
    "data-lightweight-kind": kind,
    "data-semantic-testid": semanticTestId,
    "data-node-id": node.id,
    "data-world": world,
    "data-world-render": "lightweight",
    className: "ct-canvas-node",
    style: {
      ...frameBase,
      ...style,
      ...(tagFilterDimmed ? { opacity: 0.22, pointerEvents: "none" as const } : {}),
    },
  };

  if (kind === "task") {
    const m = model as TaskModel | undefined;
    if (!m?.task) return null;
    const task = m.task;
    const badgeColor = TASK_STATUS_COLORS[task.status];
    const displayStatus = task.status_label?.trim() || statusLabel(task.status);
    const resolved = resolveTags(task.tags, m.tagFacets);
    const accent = resolved.primary?.color;
    if (world === "W0") {
      return (
        <BigKindGlance
          shared={shared}
          label="Task"
          accent={accent ?? badgeColor}
          extra={{ "data-task-id": task.id, onMouseEnter: () => onHover?.(task.id), onMouseLeave: () => onHover?.(null) }}
        />
      );
    }
    return (
      <div
        {...shared}
        data-task-id={task.id}
        data-tag-filter={tagFilterDimmed ? "dimmed" : tagFilterHighlighted ? "highlight" : undefined}
        onMouseEnter={() => onHover?.(task.id)}
        onMouseLeave={() => onHover?.(null)}
        style={{
          ...shared.style,
          ...(accent ? { borderLeftWidth: 3, borderLeftStyle: "solid", borderLeftColor: accent } : null),
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "12px 14px", minWidth: 0, height: "100%", justifyContent: "center" }}>
          <span
            data-testid="canvas-task-name"
            data-task-name-full={task.name}
            data-canvas-selectable
            className="ct-canvas-text"
            style={{
              minWidth: 0,
              fontWeight: 800,
              fontSize: `${Math.max(18, Math.round(glanceFont(style) * 0.6))}px`,
              lineHeight: 1.15,
              display: "-webkit-box",
              WebkitLineClamp: 2,
              WebkitBoxOrient: "vertical",
              overflow: "hidden",
            }}
          >
            {task.name}
          </span>
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", minWidth: 0 }}>
            <StatusChip color={badgeColor} label={displayStatus} />
            {resolved.primary && (
              <span
                style={{
                  fontSize: "0.78em",
                  fontWeight: 600,
                  padding: "0 0.55em",
                  lineHeight: 1.7,
                  borderRadius: 999,
                  background: "var(--ct-bg-active)",
                  color: accent ?? "var(--ct-fg-subtle)",
                  whiteSpace: "nowrap",
                }}
              >
                {resolved.primary.label}
              </span>
            )}
          </div>
        </div>
      </div>
    );
  }

  const plan = model as PlanModel | undefined;
  if (!plan) return null;
  const isPlan = kind === "plan";
  if (world === "W0") {
    const tags = (plan.tag_facets ?? [])
      .map((f) => (f.label || f.key || "").trim())
      .filter(Boolean);
    return (
      <BigKindGlance
        shared={shared}
        label={isPlan ? "Plan" : "Spec"}
        tags={tags}
        extra={{ "data-plan-id": plan.id }}
      />
    );
  }
  const rows = sectionRows(plan);
  const pct = Math.round(planCoverageFraction(plan) * 100);
  const driftCount = plan.driftCount ?? 0;
  const counts = { uncovered: 0, progress: 0, done: 0 };
  for (const r of rows) counts[r.glyph]++;

  return (
    <div {...shared} data-plan-id={plan.id}>
      {isPlan && (
        <div style={{ height: 3, flexShrink: 0, background: "linear-gradient(90deg, var(--ct-border-focus), rgba(52,211,153,0.85))" }} />
      )}
      <div style={{ padding: "8px 12px", display: "flex", flexDirection: "column", gap: 6, minHeight: 0, flex: 1 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          {!isPlan && <FileText size={13} style={{ flexShrink: 0, opacity: 0.7 }} />}
          <span
            data-canvas-selectable
            className="ct-canvas-text"
            style={{ fontWeight: 700, flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}
          >
            {isPlan ? plan.title : "Specification"}
          </span>
          {isPlan && driftCount > 0 && (
            <span data-testid="canvas-plan-drift-count" style={{ fontSize: "0.72em", fontWeight: 800, color: "rgba(245,158,11,0.98)", flexShrink: 0 }}>
              {driftCount} drift
            </span>
          )}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          <div title={`${pct}% covered`} style={{ flex: 1, height: 6, borderRadius: 3, background: "var(--ct-bg-active)", overflow: "hidden" }}>
            <div data-testid="canvas-plan-coverage-bar" style={{ width: `${pct}%`, height: "100%", background: pct >= 100 ? "rgba(52,211,153,0.95)" : "var(--ct-border-focus)" }} />
          </div>
          <span data-testid="canvas-world-lightweight-meta" style={{ fontSize: "0.72em", opacity: 0.75, flexShrink: 0 }}>
            {rows.length} sec · {counts.done}/{counts.progress}/{counts.uncovered}
          </span>
        </div>
      </div>
    </div>
  );
}

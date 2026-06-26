import { GitBranch, Users } from "lucide-react";
import type { Task, TaskStatus } from "../../types";
import { atLeast, type World } from "../../worlds";
import { Eyebrow } from "../provenanceShell";
import { HEARTBEAT_ACCENT, PRIORITY_TINT, STATUS_ORDER, SwarmBar, TASK_STATUS_META, taskDriver } from "./shared";

// §swarm panel: PLAN-only body filler. W0/W1 = activity bar + status counts; W2 adds
// the active-task roster (the swarm working NOW); W3 = the full roster, node-internal
// scroll (north-star #1: deep dive reveals every task, nothing truncated).
export function SwarmPanel({ tasks, world }: { tasks: Task[]; world: World }) {
  const counts = { in_progress: 0, in_review: 0, blocked: 0, pending: 0, done: 0, cancelled: 0 } as Record<TaskStatus, number>;
  for (const t of tasks) counts[t.status] = (counts[t.status] ?? 0) + 1;
  const total = tasks.length;
  const live = counts.in_progress;
  const presentOrder = STATUS_ORDER.filter((s) => counts[s] > 0);
  const structure = atLeast(world, "W1");
  const detail = atLeast(world, "W2");
  const deep = atLeast(world, "W3");

  // Roster fills the body so the card is never куцо: every task, ranked active-first
  // (in_progress → in_review → blocked → …) so the live work always leads. W1 = dot ·
  // name (structure); W2 adds priority + driver; W3 wraps names + lists all, node-
  // internal scroll. Non-deep caps to what fits the non-scrolling band.
  const rank = (t: Task) => { const i = STATUS_ORDER.indexOf(t.status); return i < 0 ? STATUS_ORDER.length : i; };
  const sorted = [...tasks].sort((a, b) => rank(a) - rank(b));
  const roster = structure ? sorted : [];
  const rosterCap = deep ? 200 : 18;
  const shownRoster = roster.slice(0, rosterCap);

  return (
    <div
      data-testid="canvas-plan-swarm"
      className="ct-canvas-fade-in"
      style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", gap: 7, padding: "8px 12px", borderTop: "1px solid var(--ct-border-subtle)" }}
    >
      <Eyebrow
        icon={<Users size={12} strokeWidth={2.2} />}
        label="Swarm"
        accent={HEARTBEAT_ACCENT}
        testid="canvas-plan-swarm-eyebrow"
        trailing={
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: "0.74em", fontWeight: 600, flexShrink: 0 }}>
            <span style={{ fontVariantNumeric: "tabular-nums", opacity: 0.8 }}>{total} task{total === 1 ? "" : "s"}</span>
            {live > 0 && (
              <span data-testid="canvas-plan-swarm-live" style={{ display: "inline-flex", alignItems: "center", gap: 4, color: HEARTBEAT_ACCENT, fontWeight: 700 }}>
                <span className="ct-canvas-heartbeat-dot" aria-hidden style={{ width: 6, height: 6, borderRadius: "50%", background: HEARTBEAT_ACCENT }} />
                {live} live
              </span>
            )}
          </span>
        }
      />
      <SwarmBar counts={counts} order={presentOrder} total={total} />
      {atLeast(world, "W1") && (
        <div data-testid="canvas-plan-swarm-counts" style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
          {presentOrder.map((s) => {
            const meta = TASK_STATUS_META[s];
            return (
              <span key={s} title={`${counts[s]} ${meta.label}`} style={{ display: "inline-flex", alignItems: "center", gap: 5, padding: "0.1em 0.55em", borderRadius: 999, fontSize: "0.72em", fontWeight: 600, border: `1px solid color-mix(in srgb, ${meta.color} 42%, transparent)`, background: `color-mix(in srgb, ${meta.color} 12%, transparent)`, color: "var(--ct-fg)", whiteSpace: "nowrap" }}>
                <span className={meta.live ? "ct-canvas-heartbeat-dot" : undefined} aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: meta.color, flexShrink: 0 }} />
                <span style={{ fontVariantNumeric: "tabular-nums", fontWeight: 700 }}>{counts[s]}</span>
                <span style={{ opacity: 0.85 }}>{meta.label}</span>
              </span>
            );
          })}
        </div>
      )}
      {shownRoster.length > 0 && (
        <div data-testid="canvas-plan-swarm-roster" style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", gap: 3, overflow: "hidden" }}>
          {shownRoster.map((t) => {
            const meta = TASK_STATUS_META[t.status];
            const driver = detail ? taskDriver(t) : "";
            return (
              <div
                key={t.id}
                data-testid="canvas-plan-swarm-task"
                data-task-id={t.id}
                data-task-status={t.status}
                role="button"
                tabIndex={0}
                title={`Fly to task: ${t.name}`}
                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.currentTarget.click(); } }}
                onMouseDown={(e) => e.stopPropagation()}
                className="ct-canvas-swarm-row"
                style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0, padding: "2px 4px", margin: "0 -4px", borderRadius: 5, cursor: "pointer" }}
              >
                <span className={meta.live ? "ct-canvas-heartbeat-dot" : undefined} aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: meta.color, flexShrink: 0 }} />
                {detail && t.priority && (
                  <span title={`Priority ${t.priority.toUpperCase()}`} style={{ flexShrink: 0, fontSize: "0.64em", fontWeight: 800, letterSpacing: "0.02em", fontFamily: "var(--ct-font-mono)", color: PRIORITY_TINT[t.priority], opacity: t.priority === "p2" || t.priority === "p3" ? 0.7 : 1 }}>{t.priority.toUpperCase()}</span>
                )}
                <span data-canvas-selectable className="ct-canvas-text" style={{ flex: 1, minWidth: 0, fontSize: "0.8em", lineHeight: 1.35, ...(deep ? { whiteSpace: "normal", wordBreak: "break-word" } : { whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }) }}>{t.name}</span>
                {driver && (
                  <span title={driver} style={{ flexShrink: 0, maxWidth: 130, display: "inline-flex", alignItems: "center", gap: 4, fontSize: "0.68em", fontWeight: 600, color: "var(--ct-fg)", opacity: 0.72, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                    <GitBranch size={9} aria-hidden style={{ flexShrink: 0, opacity: 0.7 }} />
                    {driver}
                  </span>
                )}
              </div>
            );
          })}
          {roster.length > shownRoster.length && (
            <span style={{ fontSize: "0.7em", opacity: 0.6, paddingLeft: 14 }}>+{roster.length - shownRoster.length} more</span>
          )}
        </div>
      )}
    </div>
  );
}

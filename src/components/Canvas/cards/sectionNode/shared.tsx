import { type CSSProperties } from "react";
import { Archive, Check, CircleDot, TriangleAlert } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { LoopStatus, PlanStatus, Task, TaskPriority, TaskStatus } from "../../types";
import { atLeast, type World } from "../../worlds";
import type { SectionRowGlyph } from "../../sectionRows";

// §24.3 / §32: SPEC and PLAN are two connected sectioned nodes (variant), each a
// FIXED frame whose CONTENT adapts to the node's appW WORLD band (§26.2):
//   W0 mass     — header + coverage + top-3 section rows + status counts (§32.2 —
//                 COMPRESSED content, never a hollow "N sections" placeholder).
//   W1 structure— all section rows (glyph · title · count · output dot).
//   W2 content  — bodies reveal: SPEC prose markdown, PLAN excerpts + affordances.
//   W3 deep     — full bodies, node-internal scroll. SPEC owns the full prose.
export const frameStyle = (variant: "plan" | "spec"): CSSProperties => ({
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-lg)",
  background: variant === "spec" ? "var(--ct-titlebar-bg)" : "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow:
    variant === "plan"
      ? "0 10px 28px -8px rgba(0,0,0,0.45), 0 2px 6px -2px rgba(0,0,0,0.3)"
      : "var(--ct-shadow-sm)",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
});

export const GLYPH = {
  done: { Icon: Check, color: "rgba(52,211,153,1)", rail: "rgba(52,211,153,0.9)" },
  progress: { Icon: CircleDot, color: "rgba(129,140,248,1)", rail: "rgba(99,102,241,0.95)" },
  uncovered: { Icon: TriangleAlert, color: "rgba(251,191,36,1)", rail: "rgba(251,191,36,1)" },
} as const satisfies Record<SectionRowGlyph, { Icon: typeof Check; color: string; rail: string }>;

// The orchestrator's LIVE pulse, distinct from the static goal (north-star): reuse
// the product's "live presence" green (matches TaskCard's working pill) so a plan
// that is actively being driven reads the same everywhere.
export const HEARTBEAT_ACCENT = "rgba(82,180,145,0.95)";

// Loop status maps onto the SAME semantics as section-row glyphs: running = the
// "progress" indigo (+ pulse), done = green, exhausted = amber attention, stopped
// = quiet. No new color vocabulary.
export const LOOP_STATUS: Record<LoopStatus, { label: string; color: string; live: boolean }> = {
  running: { label: "running", color: "rgba(129,140,248,1)", live: true },
  done: { label: "done", color: "rgba(52,211,153,1)", live: false },
  exhausted: { label: "exhausted", color: "rgba(251,191,36,1)", live: false },
  stopped: { label: "stopped", color: "var(--ct-fg)", live: false },
};

export function iconBtn(): CSSProperties {
  return { display: "flex", alignItems: "center", justifyContent: "center", width: 22, height: 22, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}

export function smallBtn(): CSSProperties {
  return { display: "inline-flex", alignItems: "center", justifyContent: "center", width: 18, height: 18, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}

export function copyText(text: string): void {
  if (typeof navigator !== "undefined" && navigator.clipboard) void navigator.clipboard.writeText(text).catch(() => {});
}

// Structural-link blue (matches the owner dot): spec binding + lineage are the
// plan's relationships to OTHER nodes — fly-to anchors. Supersedes borrows the
// drift amber (a replacement is an attention edge), so no new color vocabulary.
export const LINK_ACCENT = "rgba(56,189,248,0.95)";
export const SUPERSEDE_ACCENT = "rgba(245,158,11,0.98)";
export const VOCAB_ACCENT = "rgba(148,163,184,0.95)";

export function metaChip(accent: string, interactive = false): CSSProperties {
  return {
    display: "inline-flex", alignItems: "center", gap: 5, maxWidth: 260,
    padding: "0.14em 0.6em", borderRadius: 999, fontSize: "0.74em", fontWeight: 600,
    border: `1px solid color-mix(in srgb, ${accent} 42%, transparent)`,
    background: `color-mix(in srgb, ${accent} 12%, transparent)`,
    color: "var(--ct-fg)", whiteSpace: "nowrap",
    cursor: interactive ? "pointer" : "default",
  };
}

export function shortId(id: string): string {
  return id.length > 14 ? `${id.slice(0, 7)}…${id.slice(-4)}` : id;
}

// Plan lifecycle at a glance: ACTIVE reuses the live presence green (+ pulse —
// north-star liveness); ARCHIVED is a quiet slate badge. Shown at every band so
// the far glance already says whether this plan is live (§32.2 meaningful far).
export function StatusChip({ status, world }: { status: PlanStatus; world: World }) {
  const archived = status === "archived";
  const accent = archived ? VOCAB_ACCENT : HEARTBEAT_ACCENT;
  return (
    <span
      data-testid="canvas-plan-status"
      data-plan-status={status}
      title={archived ? "Archived plan" : "Active plan"}
      style={{ display: "inline-flex", alignItems: "center", gap: 5, flexShrink: 0, height: 20, padding: atLeast(world, "W1") ? "0 8px 0 7px" : "0 6px", borderRadius: 999, fontSize: "0.78em", fontWeight: 700, letterSpacing: "0.01em", color: accent, background: `color-mix(in srgb, ${accent} 13%, transparent)`, border: `1px solid color-mix(in srgb, ${accent} 45%, transparent)`, whiteSpace: "nowrap" }}
    >
      {archived
        ? <Archive size={11} strokeWidth={2.2} aria-hidden />
        : <span className="ct-canvas-heartbeat-dot" aria-hidden style={{ width: 6, height: 6, borderRadius: "50%", background: accent }} />}
      {atLeast(world, "W1") && <span>{archived ? "Archived" : "Active"}</span>}
    </span>
  );
}

export function SectionMarkdownBody({ text, testid }: { text: string; testid: string }) {
  return (
    <div
      data-testid={testid}
      data-renderer="rich-markdown"
      data-canvas-selectable
      className="ct-canvas-fade-in ct-canvas-text"
      style={{ fontSize: "inherit", lineHeight: "inherit", overflowWrap: "anywhere" }}
    >
      <RichMarkdown text={text} />
    </div>
  );
}

export function CoverageBar({ pct }: { pct: number }) {
  return (
    <div title={`${pct}% covered`} style={{ flex: 1, height: 6, borderRadius: 3, background: "var(--ct-bg-active)", overflow: "hidden" }}>
      <div data-testid="canvas-plan-coverage-bar" style={{ width: `${pct}%`, height: "100%", background: pct >= 100 ? "rgba(52,211,153,0.95)" : "var(--ct-border-focus)" }} />
    </div>
  );
}

// §swarm: the plan's TASKS are the work the swarm executes — its single most
// meaningful content, yet today only a per-section count surfaces. The Swarm panel
// fills the body with live collective state (north-star: never куцо; alive). Status
// colors reuse the product's existing canvas vocabulary — no new palette. Live
// (in_progress) leads; done/cancelled recede last.
export const TASK_STATUS_META: Record<TaskStatus, { label: string; color: string; live?: boolean }> = {
  in_progress: { label: "in progress", color: "rgba(129,140,248,1)", live: true },
  in_review: { label: "in review", color: "rgba(245,158,11,0.98)" },
  blocked: { label: "blocked", color: "rgba(248,113,113,0.98)" },
  pending: { label: "pending", color: "rgba(148,163,184,0.95)" },
  done: { label: "done", color: "rgba(52,211,153,1)" },
  cancelled: { label: "cancelled", color: "rgba(148,163,184,0.62)" },
};
export const STATUS_ORDER: readonly TaskStatus[] = ["in_progress", "in_review", "blocked", "pending", "done", "cancelled"];
export const PRIORITY_TINT: Record<TaskPriority, string> = {
  p0: "rgba(248,113,113,0.98)", p1: "rgba(245,158,11,0.98)", p2: "rgba(148,163,184,0.95)", p3: "rgba(148,163,184,0.78)",
};

// Who is driving a task right now: role > peer > pane — a single compact label so
// the roster reads "what · who", the multiplayer-presence cue (north-star #2).
export function taskDriver(t: Task): string {
  return (t.taken_by_role ?? t.peer ?? t.lane?.workspace_name ?? "").trim();
}

// The signature element: a proportional activity bar of task statuses; the live
// (in_progress) segment pulses — the swarm's heartbeat at a single glance.
export function SwarmBar({ counts, order, total }: { counts: Record<TaskStatus, number>; order: readonly TaskStatus[]; total: number }) {
  return (
    <div data-testid="canvas-plan-swarm-bar" title={`${total} task${total === 1 ? "" : "s"}`} style={{ display: "flex", width: "100%", height: 7, borderRadius: 4, overflow: "hidden", background: "var(--ct-bg-active)", flexShrink: 0 }}>
      {order.map((s) => {
        const meta = TASK_STATUS_META[s];
        return (
          <span
            key={s}
            className={meta.live ? "ct-canvas-heartbeat-dot" : undefined}
            aria-hidden
            style={{ flexGrow: counts[s], flexBasis: 0, minWidth: 2, background: meta.color }}
          />
        );
      })}
    </div>
  );
}

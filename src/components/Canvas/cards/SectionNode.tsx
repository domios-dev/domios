import { useRef, type CSSProperties } from "react";
import { Activity, Archive, ArrowUpRight, Check, CircleDot, Copy, Crosshair, FileText, GitBranch, Link2, Maximize2, Milestone, Repeat, Replace, Tags, TriangleAlert, Users } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { LoopSpec, LoopStatus, OrchestratorHeartbeat, Plan, PlanStatus, Task, TaskPriority, TaskStatus } from "../types";
import type { PlanWorkspaceScope } from "../planWorkspaceScope";
import { DEFAULT_CANVAS_SETTINGS, MILESTONE_LANE_COLORS } from "../settings";
import { atLeast, type World } from "../worlds";
import { sectionRows, type SectionRow, type SectionRowGlyph } from "../sectionRows";
import { ownerName } from "../owner";
import type { SectionNavArgs } from "../nodeRegistry";
import { planCoverageFraction } from "./coverage";
import { digestMarkdown } from "./digest";
import { Eyebrow } from "./provenanceShell";
import type { CanvasCardStyleProps } from "./types";

const S = DEFAULT_CANVAS_SETTINGS;

// §24.3 / §32: SPEC and PLAN are two connected sectioned nodes (variant), each a
// FIXED frame whose CONTENT adapts to the node's appW WORLD band (§26.2):
//   W0 mass     — header + coverage + top-3 section rows + status counts (§32.2 —
//                 COMPRESSED content, never a hollow "N sections" placeholder).
//   W1 structure— all section rows (glyph · title · count · output dot).
//   W2 content  — bodies reveal: SPEC prose markdown, PLAN excerpts + affordances.
//   W3 deep     — full bodies, node-internal scroll. SPEC owns the full prose.
const frameStyle = (variant: "plan" | "spec"): CSSProperties => ({
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

const GLYPH = {
  done: { Icon: Check, color: "rgba(52,211,153,1)", rail: "rgba(52,211,153,0.9)" },
  progress: { Icon: CircleDot, color: "rgba(129,140,248,1)", rail: "rgba(99,102,241,0.95)" },
  uncovered: { Icon: TriangleAlert, color: "rgba(251,191,36,1)", rail: "rgba(251,191,36,1)" },
} as const satisfies Record<SectionRowGlyph, { Icon: typeof Check; color: string; rail: string }>;

// The orchestrator's LIVE pulse, distinct from the static goal (north-star): reuse
// the product's "live presence" green (matches TaskCard's working pill) so a plan
// that is actively being driven reads the same everywhere.
const HEARTBEAT_ACCENT = "rgba(82,180,145,0.95)";

// Loop status maps onto the SAME semantics as section-row glyphs: running = the
// "progress" indigo (+ pulse), done = green, exhausted = amber attention, stopped
// = quiet. No new color vocabulary.
const LOOP_STATUS: Record<LoopStatus, { label: string; color: string; live: boolean }> = {
  running: { label: "running", color: "rgba(129,140,248,1)", live: true },
  done: { label: "done", color: "rgba(52,211,153,1)", live: false },
  exhausted: { label: "exhausted", color: "rgba(251,191,36,1)", live: false },
  stopped: { label: "stopped", color: "var(--ct-fg)", live: false },
};

function iconBtn(): CSSProperties {
  return { display: "flex", alignItems: "center", justifyContent: "center", width: 22, height: 22, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}

function smallBtn(): CSSProperties {
  return { display: "inline-flex", alignItems: "center", justifyContent: "center", width: 18, height: 18, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}

function copyText(text: string): void {
  if (typeof navigator !== "undefined" && navigator.clipboard) void navigator.clipboard.writeText(text).catch(() => {});
}

// Structural-link blue (matches the owner dot): spec binding + lineage are the
// plan's relationships to OTHER nodes — fly-to anchors. Supersedes borrows the
// drift amber (a replacement is an attention edge), so no new color vocabulary.
const LINK_ACCENT = "rgba(56,189,248,0.95)";
const SUPERSEDE_ACCENT = "rgba(245,158,11,0.98)";
const VOCAB_ACCENT = "rgba(148,163,184,0.95)";

function metaChip(accent: string, interactive = false): CSSProperties {
  return {
    display: "inline-flex", alignItems: "center", gap: 5, maxWidth: 260,
    padding: "0.14em 0.6em", borderRadius: 999, fontSize: "0.74em", fontWeight: 600,
    border: `1px solid color-mix(in srgb, ${accent} 42%, transparent)`,
    background: `color-mix(in srgb, ${accent} 12%, transparent)`,
    color: "var(--ct-fg)", whiteSpace: "nowrap",
    cursor: interactive ? "pointer" : "default",
  };
}

function shortId(id: string): string {
  return id.length > 14 ? `${id.slice(0, 7)}…${id.slice(-4)}` : id;
}

// Plan lifecycle at a glance: ACTIVE reuses the live presence green (+ pulse —
// north-star liveness); ARCHIVED is a quiet slate badge. Shown at every band so
// the far glance already says whether this plan is live (§32.2 meaningful far).
function StatusChip({ status, world }: { status: PlanStatus; world: World }) {
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

// §linkage: the plan's edges to OTHER efforts — the spec(s) it binds to and its
// lineage (follows / supersedes). Far glance = a single spec-bound chip + lineage
// count; W2 adds the bound-section count; W3 reveals short ids. Each chip carries
// data-fly-to-* so a later iter can POST /api/canvas/focus to the linked node;
// today click yanks the id (immediately useful, fly-ready).
function LinkageStrip({ plan, world }: { plan: Plan; world: World }) {
  const specId = plan.primary_spec_id?.trim() ?? "";
  const links = plan.spec_links ?? [];
  const lineage = (plan.lineage ?? []).filter((l) => l.to?.trim());
  const boundSections = new Set(links.map((l) => l.plan_section_id)).size;
  const resolvedSpec = specId || links[0]?.spec_id || "";
  const detail = atLeast(world, "W2");
  const deep = atLeast(world, "W3");

  return (
    <div data-testid="canvas-plan-linkage" className="ct-canvas-fade-in" style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 5 }}>
      <Eyebrow icon={<Link2 size={12} strokeWidth={2.2} />} label="Linked" accent={LINK_ACCENT} testid="canvas-plan-linkage-eyebrow" />
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
        {resolvedSpec && (
          <span
            data-testid="canvas-plan-spec-link"
            data-fly-to-kind="spec"
            data-fly-to-id={resolvedSpec}
            role="button"
            tabIndex={0}
            title={`Fly to bound spec: ${resolvedSpec}${boundSections ? `\n${boundSections} section link${boundSections === 1 ? "" : "s"}` : ""}`}
            onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.currentTarget.click(); } }}
            onMouseDown={(e) => e.stopPropagation()}
            style={metaChip(LINK_ACCENT, true)}
          >
            <FileText size={11} strokeWidth={2.2} aria-hidden style={{ flexShrink: 0, opacity: 0.85 }} />
            <span style={{ fontWeight: 700 }}>Spec</span>
            {detail && boundSections > 0 && (
              <span style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.9em", opacity: 0.7 }}>{boundSections}§</span>
            )}
            {deep && <span style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.86em", opacity: 0.55 }}>{shortId(resolvedSpec)}</span>}
          </span>
        )}
        {lineage.map((link, i) => {
          const sup = link.rel === "supersedes";
          const accent = sup ? SUPERSEDE_ACCENT : LINK_ACCENT;
          return (
            <span
              key={`${link.rel}:${link.to}:${i}`}
              data-testid="canvas-plan-lineage"
              data-fly-to-kind={link.rel}
              data-fly-to-id={link.to}
              role="button"
              tabIndex={0}
              title={`Fly to ${link.rel}: ${link.to}`}
              onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.currentTarget.click(); } }}
              onMouseDown={(e) => e.stopPropagation()}
              style={metaChip(accent, true)}
            >
              {sup ? <Replace size={10} strokeWidth={2.2} aria-hidden style={{ flexShrink: 0 }} /> : <ArrowUpRight size={11} strokeWidth={2.4} aria-hidden style={{ flexShrink: 0 }} />}
              <span style={{ fontWeight: 700 }}>{link.rel}</span>
              {deep && <span style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.86em", opacity: 0.55 }}>{shortId(link.to)}</span>}
            </span>
          );
        })}
      </div>
    </div>
  );
}

// §vocabulary: the plan's DECLARED vocabularies — milestone kinds (with their lane
// colors) and tag facets. Reference material, not glance-critical: revealed inside
// the scroll body at W2 (kinds) / W3 (facets), so the deep dive shows the full
// legend via node-internal scroll and the far glance stays uncluttered.
function VocabularyLegend({ plan, world }: { plan: Plan; world: World }) {
  const kinds = plan.milestone_kinds ?? [];
  const facets = (plan.tag_facets ?? []).filter((f) => f.values.length > 0);
  const deep = atLeast(world, "W3");
  const maxKinds = deep ? 32 : 6;
  const shownKinds = kinds.slice(0, maxKinds);
  const showFacets = deep && facets.length > 0;

  return (
    <div data-testid="canvas-plan-vocab" className="ct-canvas-fade-in" style={{ flexShrink: 0, padding: "8px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 8 }}>
      {kinds.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
          <Eyebrow icon={<Milestone size={12} strokeWidth={2.2} />} label="Milestone kinds" accent={VOCAB_ACCENT} testid="canvas-plan-kinds-eyebrow" />
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
            {shownKinds.map((k, i) => {
              const color = k.color ?? MILESTONE_LANE_COLORS[i % MILESTONE_LANE_COLORS.length];
              return (
                <span key={k.key} data-testid="canvas-plan-kind" title={k.label ?? k.key} style={metaChip(color)}>
                  <span aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: color, flexShrink: 0 }} />
                  <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>{k.label ?? k.key}</span>
                </span>
              );
            })}
            {kinds.length > maxKinds && <span style={{ fontSize: "0.72em", opacity: 0.6, alignSelf: "center" }}>+{kinds.length - maxKinds} more</span>}
          </div>
        </div>
      )}
      {showFacets && facets.map((facet) => (
        <div key={facet.key} style={{ display: "flex", flexDirection: "column", gap: 5 }}>
          <Eyebrow icon={<Tags size={12} strokeWidth={2.2} />} label={facet.label ?? facet.key} accent={VOCAB_ACCENT} testid="canvas-plan-facet-eyebrow" />
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
            {facet.values.map((val) => {
              const color = val.color ?? VOCAB_ACCENT;
              return (
                <span key={val.v} data-testid="canvas-plan-facet-value" title={val.label ?? val.v} style={metaChip(color)}>
                  {val.color && <span aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: color, flexShrink: 0 }} />}
                  <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>{val.label ?? val.v}</span>
                </span>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

function SectionMarkdownBody({ text, testid }: { text: string; testid: string }) {
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

function CoverageBar({ pct }: { pct: number }) {
  return (
    <div title={`${pct}% covered`} style={{ flex: 1, height: 6, borderRadius: 3, background: "var(--ct-bg-active)", overflow: "hidden" }}>
      <div data-testid="canvas-plan-coverage-bar" style={{ width: `${pct}%`, height: "100%", background: pct >= 100 ? "rgba(52,211,153,0.95)" : "var(--ct-border-focus)" }} />
    </div>
  );
}

// §heartbeat: the orchestrator's PRESENT tense — current focus + the lanes it is
// driving right now, with operating rules (W2+) and recent learnings (W3) below.
// The goal strip above is the destination; this strip is the live pulse toward it.
function HeartbeatStrip({ hb, world }: { hb: OrchestratorHeartbeat; world: World }) {
  const focus = hb.current_focus?.trim() ?? "";
  const lanes = (hb.active_lanes ?? []).map((l) => l.trim()).filter(Boolean);
  const rules = atLeast(world, "W2") ? (hb.rules ?? []).map((r) => r.trim()).filter(Boolean) : [];
  const learnings = atLeast(world, "W3") ? (hb.learnings ?? []).filter((l) => l.text?.trim()) : [];
  const showLanes = atLeast(world, "W1") && lanes.length > 0;
  const maxLanes = 5;

  return (
    <div
      data-testid="canvas-plan-heartbeat"
      className="ct-canvas-fade-in"
      style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", background: `color-mix(in srgb, ${HEARTBEAT_ACCENT} 7%, transparent)`, display: "flex", flexDirection: "column", gap: 5 }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
        <span aria-hidden style={{ display: "inline-flex", alignItems: "center", flexShrink: 0, color: HEARTBEAT_ACCENT }}>
          <span className="ct-canvas-heartbeat-dot" style={{ width: 7, height: 7, borderRadius: "50%", background: HEARTBEAT_ACCENT, marginRight: 5 }} />
          <Activity size={12} strokeWidth={2.4} />
        </span>
        <span style={{ fontSize: "0.62em", fontWeight: 800, letterSpacing: "0.14em", textTransform: "uppercase", color: HEARTBEAT_ACCENT, flexShrink: 0 }}>Now</span>
        {focus && (
          <span
            data-testid="canvas-plan-focus"
            title={focus}
            data-canvas-selectable
            className="ct-canvas-text"
            style={{ flex: 1, minWidth: 0, fontSize: "0.82em", fontWeight: 600, lineHeight: 1.3, display: "-webkit-box", WebkitLineClamp: atLeast(world, "W1") ? 2 : 1, WebkitBoxOrient: "vertical", overflow: "hidden" }}
          >
            {focus}
          </span>
        )}
      </div>
      {showLanes && (
        <div data-testid="canvas-plan-lanes" style={{ display: "flex", flexWrap: "wrap", gap: 5, paddingLeft: 1 }}>
          {lanes.slice(0, maxLanes).map((lane) => (
            <span
              key={lane}
              data-testid="canvas-plan-lane"
              title={lane}
              style={{ display: "inline-flex", alignItems: "center", gap: 4, maxWidth: 160, fontSize: "0.72em", fontWeight: 600, padding: "0.05em 0.5em", borderRadius: 999, border: `1px solid color-mix(in srgb, ${HEARTBEAT_ACCENT} 45%, transparent)`, background: `color-mix(in srgb, ${HEARTBEAT_ACCENT} 13%, transparent)`, color: "var(--ct-fg)", whiteSpace: "nowrap" }}
            >
              <GitBranch size={10} aria-hidden style={{ flexShrink: 0, opacity: 0.8 }} />
              <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>{lane}</span>
            </span>
          ))}
          {lanes.length > maxLanes && <span style={{ fontSize: "0.7em", opacity: 0.6, alignSelf: "center" }}>+{lanes.length - maxLanes}</span>}
        </div>
      )}
      {rules.length > 0 && (
        <ul data-testid="canvas-plan-rules" style={{ margin: 0, paddingLeft: 16, fontSize: "0.76em", lineHeight: 1.4, opacity: 0.92, display: "flex", flexDirection: "column", gap: 2 }}>
          {rules.slice(0, 4).map((r, i) => (
            <li key={i} style={{ overflowWrap: "anywhere" }}>{r}</li>
          ))}
          {rules.length > 4 && <li style={{ listStyle: "none", opacity: 0.6 }}>+{rules.length - 4} more</li>}
        </ul>
      )}
      {learnings.length > 0 && (
        <div data-testid="canvas-plan-learnings" style={{ display: "flex", flexDirection: "column", gap: 3, marginTop: 1 }}>
          {learnings.slice(-3).reverse().map((l) => (
            <div key={l.id} style={{ display: "flex", gap: 5, fontSize: "0.75em", lineHeight: 1.35 }}>
              <span aria-hidden style={{ flexShrink: 0, opacity: 0.7 }}>💡</span>
              <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>
                {l.tag && <span style={{ fontWeight: 700, opacity: 0.8, marginRight: 4 }}>{l.tag}</span>}
                {l.text}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function LoopPill({ loop, showDetail }: { loop: LoopSpec; showDetail: boolean }) {
  const meta = LOOP_STATUS[loop.status] ?? LOOP_STATUS.stopped;
  const stages = showDetail ? loop.stages.map((s) => s.role || s.key).filter(Boolean) : [];
  const title = [
    `Loop: ${loop.target}`,
    `Status: ${meta.label}`,
    `Iterations: ${loop.count}${loop.budget > 0 ? ` / ${loop.budget}` : ""}`,
    loop.stop_when ? `Stop when: ${loop.stop_when}` : null,
    stages.length ? `Stages: ${stages.join(" › ")}` : null,
  ].filter(Boolean).join("\n");
  return (
    <span
      data-testid="canvas-plan-loop"
      data-loop-status={loop.status}
      title={title}
      style={{ display: "inline-flex", alignItems: "center", gap: 6, maxWidth: showDetail ? 300 : 210, padding: "0.12em 0.55em", borderRadius: 999, fontSize: "0.76em", border: `1px solid color-mix(in srgb, ${meta.color} 45%, transparent)`, background: `color-mix(in srgb, ${meta.color} 12%, transparent)`, color: "var(--ct-fg)", whiteSpace: "nowrap" }}
    >
      <span className={meta.live ? "ct-canvas-heartbeat-dot" : undefined} aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: meta.color, flexShrink: 0 }} />
      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", fontWeight: 600 }}>{loop.target}</span>
      {loop.budget > 0 && (
        <span data-testid="canvas-plan-loop-count" style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.92em", opacity: 0.8, flexShrink: 0 }}>
          {loop.count}/{loop.budget}
        </span>
      )}
      {stages.length > 0 && (
        <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", opacity: 0.6, fontSize: "0.9em" }}>{stages.join(" › ")}</span>
      )}
    </span>
  );
}

// §loops: surface the orchestrator's iteration engines. W0 = a one-line count/running
// summary (never a hollow strip); W1+ = a pill per loop (target · count/budget · status
// dot), revealing pipeline stages + stop-when at W2.
function LoopsStrip({ loops, world }: { loops: LoopSpec[]; world: World }) {
  const running = loops.filter((l) => l.status === "running").length;
  const showDetail = atLeast(world, "W2");
  const maxLoops = 4;

  if (!atLeast(world, "W1")) {
    return (
      <div data-testid="canvas-plan-loops" style={{ flexShrink: 0, padding: "5px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", alignItems: "center", gap: 6, fontSize: "0.74em", fontWeight: 600 }}>
        <Repeat size={11} strokeWidth={2.2} style={{ opacity: 0.7, flexShrink: 0 }} aria-hidden />
        <span>{loops.length} loop{loops.length === 1 ? "" : "s"}</span>
        {running > 0 && <span style={{ color: LOOP_STATUS.running.color }}>· {running} running</span>}
      </div>
    );
  }
  return (
    <div data-testid="canvas-plan-loops" className="ct-canvas-fade-in" style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
      <Repeat size={12} strokeWidth={2.2} style={{ opacity: 0.6, flexShrink: 0 }} aria-hidden />
      {loops.slice(0, maxLoops).map((loop) => (
        <LoopPill key={loop.id} loop={loop} showDetail={showDetail} />
      ))}
      {loops.length > maxLoops && <span style={{ fontSize: "0.72em", opacity: 0.6, alignSelf: "center" }}>+{loops.length - maxLoops} more</span>}
    </div>
  );
}

// §swarm: the plan's TASKS are the work the swarm executes — its single most
// meaningful content, yet today only a per-section count surfaces. The Swarm panel
// fills the body with live collective state (north-star: never куцо; alive). Status
// colors reuse the product's existing canvas vocabulary — no new palette. Live
// (in_progress) leads; done/cancelled recede last.
const TASK_STATUS_META: Record<TaskStatus, { label: string; color: string; live?: boolean }> = {
  in_progress: { label: "in progress", color: "rgba(129,140,248,1)", live: true },
  in_review: { label: "in review", color: "rgba(245,158,11,0.98)" },
  blocked: { label: "blocked", color: "rgba(248,113,113,0.98)" },
  pending: { label: "pending", color: "rgba(148,163,184,0.95)" },
  done: { label: "done", color: "rgba(52,211,153,1)" },
  cancelled: { label: "cancelled", color: "rgba(148,163,184,0.62)" },
};
const STATUS_ORDER: readonly TaskStatus[] = ["in_progress", "in_review", "blocked", "pending", "done", "cancelled"];
const PRIORITY_TINT: Record<TaskPriority, string> = {
  p0: "rgba(248,113,113,0.98)", p1: "rgba(245,158,11,0.98)", p2: "rgba(148,163,184,0.95)", p3: "rgba(148,163,184,0.78)",
};

// Who is driving a task right now: role > peer > pane — a single compact label so
// the roster reads "what · who", the multiplayer-presence cue (north-star #2).
function taskDriver(t: Task): string {
  return (t.taken_by_role ?? t.peer ?? t.lane?.workspace_name ?? "").trim();
}

// The signature element: a proportional activity bar of task statuses; the live
// (in_progress) segment pulses — the swarm's heartbeat at a single glance.
function SwarmBar({ counts, order, total }: { counts: Record<TaskStatus, number>; order: readonly TaskStatus[]; total: number }) {
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

// §swarm panel: PLAN-only body filler. W0/W1 = activity bar + status counts; W2 adds
// the active-task roster (the swarm working NOW); W3 = the full roster, node-internal
// scroll (north-star #1: deep dive reveals every task, nothing truncated).
function SwarmPanel({ tasks, world }: { tasks: Task[]; world: World }) {
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

export interface SectionNodeProps extends CanvasCardStyleProps {
  variant: "plan" | "spec";
  plan: Plan;
  world: World;
  driftCount?: number;
  workspaceScope?: PlanWorkspaceScope;
  /** §34 crisp text: >1 → re-rasterize at on-screen size (zoom + counter-scale). */
  crispScale?: number;
  onArchive?: () => void;
  onFitNode?: () => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  suppressRichContent?: boolean;
  richOverlay?: boolean;
}

export function SectionNode({ variant, plan, style, world, driftCount = 0, workspaceScope, onArchive, onFitNode, onSectionClick, onSectionFrame, suppressRichContent = false, richOverlay = false }: SectionNodeProps) {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const rows = sectionRows(plan);
  const pct = Math.round(planCoverageFraction(plan) * 100);
  const owner = ownerName(plan.owner);
  const isPlan = variant === "plan";
  const testid = isPlan ? "canvas-plan-node" : "canvas-spec-node";
  const participantTitle = workspaceScope?.participantWorkspaceLabels.length
    ? `Participating workspaces: ${workspaceScope.participantWorkspaceLabels.join(", ")}`
    : undefined;
  const showRows = atLeast(world, "W1");
  const budgetRich = suppressRichContent && !richOverlay;
  const showBody = atLeast(world, "W2") && !budgetRich; // the reveal band (§26.2)
  const scroll = showBody;

  const counts = { uncovered: 0, progress: 0, done: 0 };
  for (const r of rows) counts[r.glyph]++;

  // Heartbeat/loops are PLAN-only live state, rendered only when present (§render-
  // when-present). Heartbeat gates per-band so a rules/learnings-only beat never
  // shows a hollow strip below its reveal band.
  const hb = plan.heartbeat;
  const hbFocus = hb?.current_focus?.trim() ?? "";
  const hbLanes = (hb?.active_lanes ?? []).filter((l) => l.trim());
  const hbRules = (hb?.rules ?? []).filter((r) => r.trim());
  const hbLearnings = (hb?.learnings ?? []).filter((l) => l.text?.trim());
  const showHeartbeat = isPlan && !!hb && (
    !!hbFocus ||
    (atLeast(world, "W1") && hbLanes.length > 0) ||
    (atLeast(world, "W2") && hbRules.length > 0) ||
    (atLeast(world, "W3") && hbLearnings.length > 0)
  );
  const loops = plan.loops ?? [];
  const showLoops = isPlan && loops.length > 0;

  // Linkage = the plan's edges to other efforts (spec binding + lineage); vocab =
  // its declared milestone-kind / tag-facet legends. Both PLAN-only, render-when-
  // present. Linkage is glanceable (all bands); vocab reveals inside the body at W2+.
  const hasSpecLink = !!plan.primary_spec_id?.trim() || (plan.spec_links?.length ?? 0) > 0;
  const hasLineage = (plan.lineage ?? []).some((l) => l.to?.trim());
  const showLinkage = isPlan && (hasSpecLink || hasLineage);
  const hasVocab = (plan.milestone_kinds?.length ?? 0) > 0 || (plan.tag_facets ?? []).some((f) => f.values.length > 0);
  const showVocab = isPlan && showBody && hasVocab;

  // §swarm: the plan's tasks fill the body as live collective state. When present
  // the section index sits at natural height and the swarm panel takes the slack —
  // killing the empty void a sparse section list used to leave (north-star: never
  // куцо). PLAN-only; spec keeps its prose-driven layout untouched.
  const tasks = plan.tasks ?? [];
  const showSwarm = isPlan && tasks.length > 0;
  const liveTaskCount = tasks.reduce((n, t) => n + (t.status === "in_progress" ? 1 : 0), 0);

  const card = (boxStyle: CSSProperties) => (
    <div data-testid={testid} data-plan-id={plan.id} data-plan-status={plan.status} data-world={world} className="ct-canvas-node" style={{ ...frameStyle(variant), ...boxStyle }}>
      {isPlan && <div style={{ height: 3, flexShrink: 0, background: "linear-gradient(90deg, var(--ct-border-focus), rgba(52,211,153,0.85))" }} />}

      <div style={{ height: S.planHeaderHeight - (isPlan ? 3 : 0), boxSizing: "border-box", padding: "8px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 6, flexShrink: 0 }}>
        {/* §header: title owns the full first line (no inline chip/button competing
            for width); drift + node actions drop to the coverage meta row below —
            same "title is the identity" principle the task cards follow. */}
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          {!isPlan && <FileText size={13} style={{ flexShrink: 0, opacity: 0.7 }} />}
          <span title={plan.title} data-canvas-selectable className="ct-canvas-title ct-canvas-text" style={{ fontWeight: 700, flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", letterSpacing: "-0.01em" }}>
            {isPlan ? plan.title : "Specification"}
          </span>
        </div>
        {isPlan ? (
          <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "0.86em", minWidth: 0 }}>
            <StatusChip status={plan.status} world={world} />
            {owner && (
              <span data-testid="canvas-plan-owner" title={workspaceScope?.ownerWorkspaceLabel ? `${owner} @ ${workspaceScope.ownerWorkspaceLabel}` : owner} style={{ display: "flex", alignItems: "center", gap: 5, maxWidth: 190, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", padding: "1px 8px 1px 6px", borderRadius: 999, background: "var(--ct-bg-selected)", border: "1px solid var(--ct-border-subtle)" }}>
                <span style={{ width: 6, height: 6, borderRadius: "50%", background: "rgba(56,189,248,0.95)", flexShrink: 0 }} />
                {owner}
                {workspaceScope?.ownerWorkspaceLabel && <span style={{ opacity: "var(--ct-text-secondary-opacity)" }}>@ {workspaceScope.ownerWorkspaceLabel}</span>}
              </span>
            )}
            {workspaceScope?.isCrossWorkspace && (
              <span data-testid="canvas-plan-cross-workspace" title={participantTitle} style={{ display: "inline-flex", alignItems: "center", height: 20, padding: "0 7px", borderRadius: 999, fontSize: "0.78em", fontWeight: 800, letterSpacing: "0.01em", color: "rgba(129,140,248,0.98)", background: "color-mix(in srgb, rgba(129,140,248,0.98) 13%, transparent)", border: "1px solid color-mix(in srgb, rgba(129,140,248,0.98) 48%, transparent)", whiteSpace: "nowrap", flexShrink: 0 }}>
                cross-workspace
              </span>
            )}
            <CoverageBar pct={pct} />
            <span data-testid="canvas-plan-coverage-pct" style={{ fontVariantNumeric: "tabular-nums", flexShrink: 0 }}>{pct}%</span>
            <span
              data-testid={driftCount > 0 ? "canvas-plan-drift-count" : "canvas-plan-drift-slot"}
              title={driftCount > 0 ? `${driftCount} drift item${driftCount === 1 ? "" : "s"}` : undefined}
              style={{
                minWidth: 48,
                height: 20,
                padding: "0 7px",
                borderRadius: 999,
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: "0.86em",
                fontWeight: 800,
                color: "rgba(245,158,11,0.98)",
                background: "color-mix(in srgb, rgba(245,158,11,0.98) 13%, transparent)",
                border: "1px solid color-mix(in srgb, rgba(245,158,11,0.98) 50%, transparent)",
                opacity: driftCount > 0 ? 1 : 0,
                visibility: driftCount > 0 ? "visible" : "hidden",
                flexShrink: 0,
              }}
            >
              {driftCount} drift
            </span>
            {onFitNode && (
              <button type="button" data-testid="canvas-plan-fit" title="Focus this plan" onClick={(e) => { e.stopPropagation(); onFitNode(); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
                <Maximize2 size={12} />
              </button>
            )}
            {onArchive && (
              <button type="button" data-testid="canvas-plan-archive" title="Archive plan" onClick={(e) => { e.stopPropagation(); onArchive(); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
                <Archive size={12} />
              </button>
            )}
          </div>
        ) : (
          <span style={{ fontSize: "0.76em", opacity: "var(--ct-text-tertiary-opacity)" }}>{rows.length} section{rows.length === 1 ? "" : "s"}</span>
        )}
      </div>

      {isPlan && plan.goal?.vector && (
        <div
          data-testid="canvas-plan-goal"
          data-canvas-selectable
          className="ct-canvas-text"
          style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", background: "color-mix(in srgb, var(--ct-border-focus) 9%, transparent)", fontSize: "0.82em", lineHeight: 1.3, display: "flex", gap: 6, alignItems: "flex-start" }}
        >
          <span style={{ flexShrink: 0 }} title="Goal (north-star)">🎯</span>
          <span title={plan.goal.vector} style={{ display: "-webkit-box", WebkitLineClamp: atLeast(world, "W1") ? 3 : 1, WebkitBoxOrient: "vertical", overflow: "hidden", fontWeight: 600, letterSpacing: "-0.01em" }}>
            {plan.goal.vector}
          </span>
        </div>
      )}

      {showLinkage && <LinkageStrip plan={plan} world={world} />}
      {showHeartbeat && hb && <HeartbeatStrip hb={hb} world={world} />}
      {showLoops && <LoopsStrip loops={loops} world={world} />}

      <div
        ref={bodyRef}
        className="ct-canvas-scroll"
        data-testid={isPlan ? "canvas-plan-body" : "canvas-spec-body"}
        data-content={showBody ? "body" : "index"}
        data-canvas-scroll-body={scroll ? "1" : undefined}
        style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", overflowY: scroll ? "auto" : "hidden" }}
      >
        {rows.length === 0 ? (
          // No sections: the swarm IS the plan's content — render it full, never a
          // bare "N standalone tasks" line in a void.
          showSwarm ? (
            <SwarmPanel tasks={tasks} world={world} />
          ) : (
            <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", fontSize: "0.9em", opacity: "var(--ct-text-tertiary-opacity)" }}>
              {isPlan ? "no tasks yet" : "no spec sections"}
            </div>
          )
        ) : showRows ? (
          <>
            {showVocab && <VocabularyLegend plan={plan} world={world} />}
            {rows.map((row) => (
              <SectionRowView key={row.section.id} row={row} variant={variant} world={world} planId={plan.id} onSectionClick={onSectionClick} onSectionFrame={onSectionFrame} suppressRichContent={budgetRich} stretchEmpty={!showSwarm} />
            ))}
            {showSwarm && <SwarmPanel tasks={tasks} world={world} />}
          </>
        ) : (
          // §32.2 W0: COMPRESSED — top section rows + status counts + swarm pulse,
          // never hollow (north-star: meaningful far glance).
          <div data-testid="canvas-section-compressed" style={{ flex: 1, display: "flex", flexDirection: "column", gap: 5, padding: "10px 10px", justifyContent: "flex-start" }}>
            {rows.slice(0, 3).map((row) => {
              const g = GLYPH[row.glyph];
              return (
                <div key={row.section.id} style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
                  <g.Icon size={11} color={g.color} style={{ flexShrink: 0 }} strokeWidth={2.2} />
                  <span style={{ flex: 1, minWidth: 0, fontSize: "0.8em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{row.section.title}</span>
                  {isPlan && <span style={{ fontSize: "0.66em", fontFamily: "var(--ct-font-mono)", opacity: 0.65 }}>{row.taskCount}</span>}
                </div>
              );
            })}
            {rows.length > 3 && <span style={{ fontSize: "0.68em", opacity: 0.6, paddingLeft: 17 }}>+{rows.length - 3} more</span>}
            <div data-testid="canvas-section-counts" style={{ display: "flex", gap: 8, marginTop: 4, fontSize: "0.68em", fontWeight: 600 }}>
              {counts.progress > 0 && <span style={{ color: GLYPH.progress.color }}>● {counts.progress} active</span>}
              {counts.uncovered > 0 && <span style={{ color: GLYPH.uncovered.color }}>▲ {counts.uncovered} uncovered</span>}
              {counts.done > 0 && <span style={{ color: GLYPH.done.color }}>✓ {counts.done} done</span>}
            </div>
            {showSwarm && (
              <div data-testid="canvas-plan-swarm-glance" style={{ display: "flex", alignItems: "center", gap: 7, marginTop: 5, minWidth: 0 }}>
                <span style={{ flex: 1, minWidth: 0, display: "flex" }}>
                  <SwarmBar counts={tasks.reduce((c, t) => { c[t.status] = (c[t.status] ?? 0) + 1; return c; }, { in_progress: 0, in_review: 0, blocked: 0, pending: 0, done: 0, cancelled: 0 } as Record<TaskStatus, number>)} order={STATUS_ORDER} total={tasks.length} />
                </span>
                <span style={{ flexShrink: 0, fontSize: "0.66em", fontWeight: 700, color: liveTaskCount > 0 ? HEARTBEAT_ACCENT : "var(--ct-fg)", opacity: liveTaskCount > 0 ? 1 : 0.7, whiteSpace: "nowrap" }}>
                  {liveTaskCount > 0 ? `${liveTaskCount} live` : `${tasks.length} task${tasks.length === 1 ? "" : "s"}`}
                </span>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );

  // §34 superseded: world geometry stays under the single camera transform; the
  // per-card zoom/counter-scale wrapper reflowed internals and drifted at zoom steps.
  return card(style);
}

// One section. PLAN row = index (glyph·title·count·output dot) + a CLAMPED excerpt
// at W2+ (no W2→W3 toggle). SPEC block = title + 1-line abstract at W1, then the
// DEEPEST (unclamped) prose layout the moment it reveals at W2 — §22.1 reserve-at-
// deepest applied to content: W2→W3 changes only rasterization, never geometry.
// §32.7: single-click highlights connected tasks; double-click / target button
// frames them; copy-id yanks the section id. All camera motion is user-initiated.
function SectionRowView({ row, variant, world, planId, onSectionClick, onSectionFrame, suppressRichContent = false, stretchEmpty = true }: {
  row: SectionRow; variant: "plan" | "spec"; world: World; planId: string;
  onSectionClick?: (a: SectionNavArgs) => void; onSectionFrame?: (a: SectionNavArgs) => void;
  suppressRichContent?: boolean; stretchEmpty?: boolean;
}) {
  const g = GLYPH[row.glyph];
  const isPlan = variant === "plan";
  const hasBody = row.section.body.trim().length > 0;
  const showBody = atLeast(world, "W2") && hasBody && !suppressRichContent;
  const showAbstract = !isPlan && !showBody && atLeast(world, "W1") && hasBody && !suppressRichContent;
  const deep = atLeast(world, "W3");
  const clamp = isPlan && !deep; // §22.1 spec unclamped from W2; PLAN excerpt clamps at W2 but opens FULLY at W3 (deep read)
  const grow = showBody || showAbstract;
  const nav: SectionNavArgs = { planId, sectionId: row.section.id, variant };
  const showActions = atLeast(world, "W2");

  return (
    <div
      data-testid={isPlan ? "canvas-plan-row" : "canvas-spec-block"}
      data-section-id={row.section.id}
      data-row-glyph={row.glyph}
      onClick={onSectionClick ? (e) => { e.stopPropagation(); onSectionClick(nav); } : undefined}
      onDoubleClick={onSectionFrame ? (e) => { e.stopPropagation(); onSectionFrame(nav); } : undefined}
      style={{
        flex: grow || !stretchEmpty ? "0 0 auto" : 1,
        minHeight: grow || !stretchEmpty ? 36 : 0,
        position: "relative",
        display: "flex",
        flexDirection: "column",
        gap: 4,
        padding: grow ? "8px 12px" : "0 4px 0 8px",
        justifyContent: "center",
        borderLeft: `2px solid ${g.rail}`,
        borderBottom: grow ? "1px solid var(--ct-border-subtle)" : "none",
        overflow: "hidden",
        cursor: onSectionClick ? "pointer" : undefined,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: row.depth * 8 }}>
        <g.Icon size={12} color={g.color} style={{ flexShrink: 0 }} strokeWidth={row.glyph === "uncovered" ? 2.4 : 2.2} />
        <span data-canvas-selectable className="ct-canvas-title ct-canvas-text" style={{ flex: 1, minWidth: 0, fontSize: "0.84em", fontWeight: row.glyph === "uncovered" ? 600 : 500, ...(deep ? { whiteSpace: "normal", wordBreak: "break-word" } : { whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }) }}>
          {row.section.title}
        </span>
        {isPlan && <span data-testid="canvas-plan-row-count" style={{ flexShrink: 0, fontSize: "0.68em", fontFamily: "var(--ct-font-mono)", fontWeight: 600, opacity: 0.7, paddingRight: showActions ? 0 : 8 }}>{row.taskCount}</span>}
        {showActions && (
          <span style={{ display: "inline-flex", gap: 4, flexShrink: 0 }}>
            {onSectionFrame && (
              <button type="button" data-testid="canvas-section-goto" title={isPlan ? "Frame connected tasks" : "Frame mapped plan/tasks"} onClick={(e) => { e.stopPropagation(); onSectionFrame(nav); }} onMouseDown={(e) => e.stopPropagation()} style={smallBtn()}>
                <Crosshair size={11} />
              </button>
            )}
            <button type="button" data-testid="canvas-section-copy-id" title="Copy section id" onClick={(e) => { e.stopPropagation(); copyText(row.section.id); }} onMouseDown={(e) => e.stopPropagation()} style={smallBtn()}>
              <Copy size={11} />
            </button>
          </span>
        )}
      </div>
      {showAbstract && (
        <div data-testid="canvas-spec-abstract" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.78em", lineHeight: 1.4, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", paddingLeft: row.depth * 8 }}>
          {digestMarkdown(row.section.body, 80)}
        </div>
      )}
      {showBody && (
        <div
          style={clamp
            ? { fontSize: "0.82em", lineHeight: 1.5, paddingLeft: row.depth * 8, display: "-webkit-box", WebkitLineClamp: isPlan ? 4 : 8, WebkitBoxOrient: "vertical", overflow: "hidden" }
            : { fontSize: "0.82em", lineHeight: 1.55, paddingLeft: row.depth * 8 }}
        >
          <SectionMarkdownBody
            text={row.section.body}
            testid={isPlan ? "canvas-plan-section-body" : "canvas-spec-section-body"}
          />
        </div>
      )}
      {/* Per-section anchor dot on the node's right edge: PLAN output → tasks; SPEC → plan row. */}
      <span data-testid={isPlan ? "canvas-plan-output" : "canvas-spec-output"} aria-hidden style={{ position: "absolute", right: -1, top: grow ? 14 : "50%", transform: grow ? "none" : "translateY(-50%)", width: 7, height: 7, borderRadius: "50%", background: g.rail, border: "1px solid var(--ct-bg)" }} />
    </div>
  );
}

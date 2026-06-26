import type { CSSProperties } from "react";
import { ArrowRight, ArrowLeftRight, RotateCcw } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { atLeast, type World } from "../worlds";
import type { CanvasElement } from "../types";

// §32.8 handoff render: a reset COIN on the orchestrator timeline lane. Content-LOD
// W0 coin → W1 goal+timestamp → W2 next_actions + in-flight agents + session chips
// → W3 expanded body + ack state. Latest/pending PULSES; acked RECEDES spatially
// (smaller/behind), never content-faded (§32.1 — opacity is not a status signal).

export interface HandoffContent {
  seq?: number;
  goal?: string;
  mode?: string;
  ts?: number;
  reseed_state?: string;
  resume_prompt?: string;
  next_actions?: string[];
  foundational_rules?: string[];
  open_decisions?: string[];
  in_flight?: { task_id: string; task_handle?: string; status?: string; assigned_pane_id?: string | null; peer?: string | null }[];
  plan_snapshot?: { plan_title?: string; done?: number; total?: number; coverage_pct?: number };
}

function fmtTime(ts?: number): string {
  if (!ts) return "";
  try {
    return new Date(ts).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  } catch {
    return "";
  }
}

const coinStyle = (acked: boolean): CSSProperties => ({
  position: "absolute",
  boxSizing: "border-box",
  borderRadius: "var(--ct-radius-lg)",
  border: `1px solid ${acked ? "var(--ct-border-subtle)" : "var(--ct-border-focus)"}`,
  background: "color-mix(in srgb, var(--ct-titlebar-bg) 94%, var(--ct-border-focus))",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow: acked ? "var(--ct-shadow-sm)" : "0 8px 26px -10px color-mix(in srgb, var(--ct-border-focus) 60%, transparent)",
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
});

function ChipRow({ label, items, color }: { label: string; items: string[]; color?: string }) {
  if (!items.length) return null;
  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 5, alignItems: "center" }}>
      <span style={{ fontSize: "0.66em", fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.03em", opacity: 0.7 }}>{label}</span>
      {items.map((it, i) => (
        <span key={i} data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.72em", padding: "0.05em 0.45em", borderRadius: 999, border: `1px solid ${color ?? "var(--ct-border-subtle)"}`, background: "var(--ct-bg-active)", color: "var(--ct-fg)", maxWidth: 200, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{it}</span>
      ))}
    </div>
  );
}

// In-flight handoff status reads green=done, amber=in-review/blocked, focus=running.
function statusAccent(status?: string): string {
  const s = (status ?? "").toLowerCase();
  if (s === "done" || s === "accepted") return "rgba(82,180,145,0.95)";
  if (s.includes("review") || s === "blocked") return "var(--ct-warning, #fbbf24)";
  return "var(--ct-border-focus)";
}

type InFlight = NonNullable<HandoffContent["in_flight"]>[number];

// Per-agent reseed rows: each agent the next session inherits, its task status,
// and the peer it pairs with — the live roster of who is mid-flight at handoff.
function InFlightRows({ items }: { items: InFlight[] }) {
  if (!items.length) return null;
  return (
    <div data-testid="canvas-handoff-agents" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <span style={{ fontSize: "0.66em", fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.03em", opacity: 0.7 }}>in flight</span>
      {items.slice(0, 8).map((t, i) => {
        const accent = statusAccent(t.status);
        const who = t.task_handle || t.assigned_pane_id || t.task_id;
        return (
          <div key={i} data-testid="canvas-handoff-agent-row" data-agent-status={t.status ?? "running"} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: "0.74em", minWidth: 0 }}>
            <span aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", flexShrink: 0, background: accent, boxShadow: `0 0 0 2px color-mix(in srgb, ${accent} 24%, transparent)` }} />
            <span data-canvas-selectable className="ct-canvas-text" style={{ fontFamily: "var(--ct-font-mono)", fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0, flex: "0 1 auto" }}>{who}</span>
            {t.peer && (
              <span style={{ display: "inline-flex", alignItems: "center", gap: 3, opacity: 0.78, flexShrink: 0 }}>
                <ArrowLeftRight size={10} style={{ opacity: 0.7 }} />
                <span style={{ fontFamily: "var(--ct-font-mono)", maxWidth: 90, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{t.peer}</span>
              </span>
            )}
            <span style={{ flex: 1, minWidth: 4 }} />
            {t.status && (
              <span style={{ fontSize: "0.86em", fontWeight: 700, padding: "0 6px", borderRadius: 999, flexShrink: 0, color: accent, background: `color-mix(in srgb, ${accent} 15%, transparent)`, border: `1px solid color-mix(in srgb, ${accent} 40%, transparent)`, whiteSpace: "nowrap" }}>{t.status.replace(/_/g, " ")}</span>
            )}
          </div>
        );
      })}
    </div>
  );
}

// done/total coverage as a labeled track — the plan's completion at this handoff.
function PlanSnapshotBar({ snap }: { snap: NonNullable<HandoffContent["plan_snapshot"]> }) {
  const done = snap.done ?? 0;
  const total = snap.total ?? 0;
  if (total <= 0) return null;
  const pct = Math.max(0, Math.min(100, Math.round((done / total) * 100)));
  return (
    <div data-testid="canvas-handoff-progress" style={{ display: "flex", flexDirection: "column", gap: 3 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 6, fontSize: "0.7em" }}>
        <span style={{ fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.03em", opacity: 0.7 }}>plan</span>
        <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", opacity: 0.85 }}>{snap.plan_title}</span>
        <span style={{ fontFamily: "var(--ct-font-mono)", fontWeight: 700 }}>{done}/{total}</span>
      </div>
      <div style={{ position: "relative", height: 5, borderRadius: 999, background: "var(--ct-bg-active)", overflow: "hidden" }}>
        <div style={{ position: "absolute", inset: 0, width: `${pct}%`, borderRadius: 999, background: "linear-gradient(90deg, color-mix(in srgb, rgba(82,180,145,0.9) 70%, var(--ct-border-focus)), rgba(82,180,145,0.95))" }} />
      </div>
    </div>
  );
}

export interface HandoffCoinProps {
  element: CanvasElement;
  style: CSSProperties;
  world: World;
}

export function HandoffCoin({ element, style, world }: HandoffCoinProps) {
  const c = (element.content ?? {}) as HandoffContent;
  const seq = c.seq ?? 0;
  const acked = (c.reseed_state ?? "") === "acked" || (c.reseed_state ?? "") === "resumed";
  const pending = !acked;
  const inFlight = c.in_flight ?? [];

  return (
    <div
      data-testid="canvas-handoff-coin"
      data-element-id={element.id}
      data-handoff-seq={seq}
      data-reseed-state={c.reseed_state ?? "created"}
      data-world={world}
      className={`ct-canvas-node${pending ? " ct-canvas-live" : ""}`}
      style={{ ...coinStyle(acked), ...style }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 6, padding: "9px 11px", minWidth: 0, overflow: "hidden", flex: 1 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
          <span aria-hidden style={{ display: "inline-flex", alignItems: "center", justifyContent: "center", width: 22, height: 22, borderRadius: "50%", flexShrink: 0, background: acked ? "var(--ct-bg-active)" : "color-mix(in srgb, var(--ct-border-focus) 22%, transparent)", border: `1.5px solid ${acked ? "var(--ct-border-subtle)" : "var(--ct-border-focus)"}`, color: "var(--ct-border-focus)" }}>
            <RotateCcw size={12} />
          </span>
          <span style={{ fontWeight: 700, fontSize: "0.84em", whiteSpace: "nowrap" }}>Handoff #{seq}</span>
          {c.mode && <span style={{ fontSize: "0.66em", fontFamily: "var(--ct-font-mono)", padding: "0 5px", borderRadius: 999, background: "var(--ct-bg-active)", border: "1px solid var(--ct-border-subtle)" }}>{c.mode}</span>}
          <span style={{ flex: 1 }} />
          {!acked && <span data-testid="canvas-handoff-pending" style={{ fontSize: "0.64em", fontWeight: 700, color: "var(--ct-border-focus)" }}>PENDING</span>}
        </div>

        {atLeast(world, "W1") && (
          <>
            {c.goal && (
              <div data-testid="canvas-handoff-goal" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.86em", lineHeight: 1.35, fontWeight: 600, display: "-webkit-box", WebkitLineClamp: atLeast(world, "W3") ? 4 : 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
                {c.goal}
              </div>
            )}
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: "0.7em", opacity: 0.75 }}>
              {c.ts != null && <span data-testid="canvas-handoff-ts">{fmtTime(c.ts)}</span>}
              {c.plan_snapshot?.coverage_pct != null && <span>· {c.plan_snapshot.coverage_pct}% covered</span>}
              {atLeast(world, "W1") && !atLeast(world, "W2") && inFlight.length > 0 && <span>· {inFlight.length} in flight</span>}
            </div>
          </>
        )}

        {/* W2 stays compact — the footprint is fixed, so the plan progress bar is the
            single highest-value glance; the verbose per-agent roster lives at W3. */}
        {atLeast(world, "W2") && !atLeast(world, "W3") && (
          <div style={{ display: "flex", flexDirection: "column", gap: 7, marginTop: 2 }}>
            <ChipRow label="next" items={(c.next_actions ?? []).slice(0, 3)} color="color-mix(in srgb, var(--ct-border-focus) 45%, transparent)" />
            {c.plan_snapshot && <PlanSnapshotBar snap={c.plan_snapshot} />}
          </div>
        )}

        {atLeast(world, "W3") && (
          <div className="ct-canvas-scroll" data-canvas-scroll-body="1" style={{ marginTop: 4, flex: 1, overflowY: "auto", minHeight: 0, display: "flex", flexDirection: "column", gap: 8 }}>
            {c.plan_snapshot && <PlanSnapshotBar snap={c.plan_snapshot} />}
            <InFlightRows items={inFlight} />
            <ChipRow label="next" items={c.next_actions ?? []} color="color-mix(in srgb, var(--ct-border-focus) 45%, transparent)" />
            {c.plan_snapshot?.plan_title && (
              <div data-testid="canvas-handoff-sessions" style={{ display: "flex", alignItems: "center", gap: 5, fontSize: "0.7em", flexWrap: "wrap" }}>
                <span style={{ padding: "0.05em 0.45em", borderRadius: 999, background: "var(--ct-bg-active)", border: "1px solid var(--ct-border-subtle)" }}>#{Math.max(0, seq - 1)}</span>
                <ArrowRight size={11} style={{ opacity: 0.6 }} />
                <span style={{ padding: "0.05em 0.45em", borderRadius: 999, background: "color-mix(in srgb, var(--ct-border-focus) 14%, transparent)", border: "1px solid color-mix(in srgb, var(--ct-border-focus) 40%, transparent)" }}>#{seq}</span>
              </div>
            )}
            <ChipRow label="rules" items={c.foundational_rules ?? []} />
            <ChipRow label="open" items={c.open_decisions ?? []} color="color-mix(in srgb, var(--ct-warning, #fbbf24) 45%, transparent)" />
            {c.resume_prompt && (
              <div data-testid="canvas-handoff-body" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.8em", lineHeight: 1.5, overflowWrap: "anywhere" }}>
                <RichMarkdown text={c.resume_prompt} />
              </div>
            )}
            <div data-testid="canvas-handoff-ack" style={{ fontSize: "0.68em", fontWeight: 700, alignSelf: "flex-start", padding: "0.1em 0.5em", borderRadius: 999, background: acked ? "color-mix(in srgb, rgba(82,180,145,0.9) 18%, transparent)" : "color-mix(in srgb, var(--ct-border-focus) 16%, transparent)", color: acked ? "rgba(82,180,145,1)" : "var(--ct-border-focus)" }}>
              {c.reseed_state ?? "created"}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

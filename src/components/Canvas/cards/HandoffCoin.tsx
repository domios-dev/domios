import type { CSSProperties } from "react";
import { ArrowRight, RotateCcw } from "lucide-react";
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
              <div data-testid="canvas-handoff-goal" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.86em", lineHeight: 1.35, fontWeight: 600, display: "-webkit-box", WebkitLineClamp: atLeast(world, "W2") ? 3 : 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
                {c.goal}
              </div>
            )}
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: "0.7em", opacity: 0.75 }}>
              {c.ts != null && <span data-testid="canvas-handoff-ts">{fmtTime(c.ts)}</span>}
              {c.plan_snapshot?.coverage_pct != null && <span>· {c.plan_snapshot.coverage_pct}% covered</span>}
            </div>
          </>
        )}

        {atLeast(world, "W2") && (
          <div style={{ display: "flex", flexDirection: "column", gap: 6, marginTop: 2 }}>
            <ChipRow label="next" items={(c.next_actions ?? []).slice(0, 5)} color="color-mix(in srgb, var(--ct-border-focus) 45%, transparent)" />
            <ChipRow label="agents" items={inFlight.map((t) => t.task_handle || t.assigned_pane_id || t.task_id).slice(0, 6)} />
            {c.plan_snapshot?.plan_title && (
              <div data-testid="canvas-handoff-sessions" style={{ display: "flex", alignItems: "center", gap: 5, fontSize: "0.7em", flexWrap: "wrap" }}>
                <span style={{ padding: "0.05em 0.45em", borderRadius: 999, background: "var(--ct-bg-active)", border: "1px solid var(--ct-border-subtle)" }}>#{Math.max(0, seq - 1)}</span>
                <ArrowRight size={11} style={{ opacity: 0.6 }} />
                <span style={{ padding: "0.05em 0.45em", borderRadius: 999, background: "color-mix(in srgb, var(--ct-border-focus) 14%, transparent)", border: "1px solid color-mix(in srgb, var(--ct-border-focus) 40%, transparent)" }}>#{seq}</span>
              </div>
            )}
          </div>
        )}

        {atLeast(world, "W3") && (
          <div className="ct-canvas-scroll" data-canvas-scroll-body="1" style={{ marginTop: 4, flex: 1, overflowY: "auto", minHeight: 0, display: "flex", flexDirection: "column", gap: 8 }}>
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

import { useCallback, useEffect, useMemo, useState } from "react";
import { Compass, CornerDownLeft, Loader, Send } from "lucide-react";
import type { ApiConfig, Pane } from "../../lib/types";
import { sendToPane } from "./canvasApi";
import {
  buildCanvasMessage,
  buildCanvasNodeIndex,
  contextChipLabel,
  resolveCanvasContext,
  toPlanContext,
  type CanvasContext,
  type OverlayRef,
} from "./canvasContext";
import { ownerName } from "./owner";
import type { Point, WorkspaceLayout } from "./layout";
import type { Plan } from "./types";

export interface CanvasComposerProps {
  api: ApiConfig;
  plans: Plan[];
  layout: WorkspaceLayout;
  viewportCenter: Point;
  panes?: Record<string, Pane>;
  // The open detail-overlay node, if any — it WINS over the centre raycast as the referent.
  overlay?: OverlayRef | null;
}

function truncate(text: string, max = 120): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

function ownerLabelOf(plan: Plan): string {
  return ownerName(plan.owner) ?? "unowned";
}

// ONE floating, context-addressed composer — reused everywhere (it stays visible OVER an
// open detail overlay, its referent switching to the open node). The referent is resolved
// by spatial RAYCAST of the viewport centre (or the open overlay node); routing follows
// that referent's plan owner, so the target orchestrator switches live as you move.
export function CanvasComposer({ api, plans, layout, viewportCenter, panes = {}, overlay = null }: CanvasComposerProps) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dropToPlan, setDropToPlan] = useState(false);

  // Built once per layout (off the frame path); the point-query runs on camera-settle.
  const index = useMemo(() => buildCanvasNodeIndex(layout), [layout]);

  const baseContext = useMemo<CanvasContext | null>(
    () => resolveCanvasContext({ overlay, plans, layout, center: viewportCenter, index }),
    [overlay, plans, layout, viewportCenter, index],
  );

  // Reset the plan-level override whenever the underlying referent changes.
  useEffect(() => { setDropToPlan(false); }, [baseContext?.kind, baseContext?.id]);

  const context = useMemo<CanvasContext | null>(() => {
    if (!baseContext) return null;
    if (dropToPlan && baseContext.kind !== "plan") return toPlanContext(baseContext, plans) ?? baseContext;
    return baseContext;
  }, [baseContext, dropToPlan, plans]);

  const plan = useMemo(() => (context ? plans.find((p) => p.id === context.planId) ?? null : null), [context, plans]);
  const paneId = plan?.owner.pane_id ?? null;
  const ownerLabel = plan ? ownerLabelOf(plan) : "";

  const ownerState = paneId ? panes[paneId]?.agent_state ?? null : null;
  const working = sending || ownerState?.state === "working";
  const activity = useMemo(() => {
    if (sending) return { working: true, text: "Sending…" };
    if (ownerState?.state === "working") return { working: true, text: truncate(ownerState.status_line?.trim() || "working on it…") };
    if (ownerState?.last_turn_summary?.trim()) return { working: false, text: truncate(ownerState.last_turn_summary) };
    return null;
  }, [sending, ownerState]);

  const canSend = !!paneId && !!context && text.trim().length > 0 && !sending;

  const send = useCallback(async () => {
    if (!paneId || !context || !text.trim()) return;
    setSending(true);
    setError(null);
    try {
      await sendToPane(api, paneId, buildCanvasMessage(text.trim(), context, plan?.title ?? context.title));
      setText("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSending(false);
    }
  }, [api, paneId, context, plan, text]);

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        if (canSend) void send();
      }
    },
    [canSend, send],
  );

  const hint = !context
    ? "Point the canvas at a plan to talk to its orchestrator"
    : !paneId
      ? `Plan "${plan?.title ?? ""}" has no orchestrator to talk to`
      : null;

  return (
    <div
      data-testid="canvas-composer"
      className="absolute bottom-4 left-1/2 z-50 flex w-[min(640px,90%)] -translate-x-1/2 flex-col gap-1.5 rounded-xl border p-2 shadow-lg"
      style={{
        background: "color-mix(in srgb, var(--ct-bg) 86%, transparent)",
        backdropFilter: "blur(10px) saturate(1.2)",
        WebkitBackdropFilter: "blur(10px) saturate(1.2)",
        borderColor: "var(--ct-border)",
        color: "var(--ct-fg)",
      }}
      onMouseDown={(e) => e.stopPropagation()}
      onWheel={(e) => e.stopPropagation()}
    >
      {/* Reply surfacing: the target orchestrator's latest reply (or live status) at top. */}
      {activity && (
        <div
          data-testid="canvas-activity-bubble"
          data-activity-working={activity.working ? "true" : "false"}
          className="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs"
          style={{ background: "var(--ct-bg)", border: `1px solid ${activity.working ? "var(--ct-border-focus)" : "var(--ct-border)"}`, color: "var(--ct-fg)" }}
          title={activity.text}
        >
          {activity.working
            ? <Loader size={11} className="ct-spin shrink-0" style={{ color: "var(--ct-border-focus)" }} />
            : <CornerDownLeft size={11} className="shrink-0" style={{ color: "var(--ct-accent)" }} />}
          <span className="truncate" style={{ opacity: "var(--ct-text-secondary-opacity)" }}>{activity.text}</span>
        </div>
      )}

      {/* Referent chip (exact node + id, click → plan-level) + the target orchestrator pill. */}
      <div className="flex items-center justify-between gap-2 px-0.5 text-xs">
        {context ? (
          <button
            type="button"
            data-testid="canvas-context-chip"
            data-context-kind={context.kind}
            data-context-id={context.id}
            data-context-plan={context.planId}
            onClick={() => { if (context.kind !== "plan") setDropToPlan(true); }}
            title={context.kind === "plan" ? "Context: the whole plan" : "Attached to this node — click to use the whole plan instead"}
            className="flex min-w-0 items-center gap-1 truncate rounded-full border px-2 py-0.5"
            style={{ borderColor: "var(--ct-border-focus)", background: "color-mix(in srgb, var(--ct-accent) 12%, transparent)", color: "var(--ct-fg)", cursor: context.kind === "plan" ? "default" : "pointer" }}
          >
            <span className="truncate">{contextChipLabel(context)}</span>
          </button>
        ) : (
          <span data-testid="canvas-context-chip" style={{ opacity: "var(--ct-text-secondary-opacity)" }}>📍 no referent</span>
        )}
        <span
          data-testid="canvas-composer-target"
          data-owner-pane={paneId ?? ""}
          className="flex shrink-0 items-center gap-1 truncate rounded-full border px-2 py-0.5"
          style={{ borderColor: paneId ? "var(--ct-border)" : "var(--ct-border-subtle)", background: "color-mix(in srgb, var(--ct-fg) 6%, transparent)", maxWidth: "45%" }}
          title={paneId ? `Talking to orchestrator ${ownerLabel}` : "No orchestrator owns this plan"}
        >
          <Compass size={11} className="shrink-0" style={{ color: working ? "var(--ct-border-focus)" : "var(--ct-accent)" }} />
          <span className="truncate">{paneId ? ownerLabel : "no orchestrator"}</span>
        </span>
      </div>

      <div className="flex items-end gap-2">
        <textarea
          data-testid="canvas-composer-input"
          value={text}
          onInput={(e) => setText((e.target as HTMLTextAreaElement).value)}
          onKeyDown={onKeyDown}
          rows={1}
          placeholder={hint ?? `Message ${ownerLabel || "orchestrator"}…`}
          className="max-h-32 min-h-[2rem] flex-1 resize-none rounded-md border bg-transparent px-2 py-1.5 text-sm outline-none"
          style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
        />
        <button
          type="button"
          data-testid="canvas-composer-send"
          onClick={() => void send()}
          disabled={!canSend}
          title={hint ?? "Send to orchestrator"}
          className="flex h-8 w-8 items-center justify-center rounded-md border disabled:opacity-40"
          style={{ background: "var(--ct-accent, var(--ct-titlebar-bg))", borderColor: "var(--ct-border)", color: "var(--ct-fg)", cursor: canSend ? "pointer" : "not-allowed" }}
        >
          <Send size={14} />
        </button>
      </div>
      {error && <div className="px-1 text-xs" style={{ color: "var(--ct-error, #f87171)" }}>{error}</div>}
    </div>
  );
}

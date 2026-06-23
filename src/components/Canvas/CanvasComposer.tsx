import { useCallback, useMemo, useState } from "react";
import { Send } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { baseUrl, jsonHeaders } from "../MemoryPanel/memoryApi";
import { useCanvasFocusTarget } from "./canvasFocusStore";
import { focusAt, type FocusResult, type Point, type WorkspaceLayout } from "./layout";
import { DEFAULT_CANVAS_SETTINGS, type CanvasLayoutSettings } from "./settings";
import type { Milestone, Plan, Section, Task } from "./types";

export interface CanvasComposerProps {
  api: ApiConfig;
  plans: Plan[];
  layout: WorkspaceLayout;
  // World-space point under the viewport center; drives focus-based targeting.
  viewportCenter: Point;
  mode: "workspace" | "focus";
  settings?: CanvasLayoutSettings;
}

interface ComposerTarget {
  plan: Plan;
  paneId: string | null;
  ownerLabel: string;
  focus: FocusResult;
}

function collectSections(sections: Section[], out: Map<string, Section>): void {
  for (const s of sections) {
    out.set(s.id, s);
    if (s.subsections.length) collectSections(s.subsections, out);
  }
}

function ownerLabelOf(plan: Plan): string {
  return plan.owner.title ?? plan.owner.pane_id ?? "unknown owner";
}

// Target = the plan whose region you're looking at (focus-store in focus mode,
// else the frame nearest the viewport center); its owner pane is the orchestrator.
function resolveTarget(
  mode: "workspace" | "focus",
  plans: Plan[],
  layout: WorkspaceLayout,
  center: Point,
  settings: CanvasLayoutSettings,
  focusPlanId: string | null,
): ComposerTarget | null {
  const fa = focusAt(layout, center, settings);
  const planId = mode === "focus" && focusPlanId ? focusPlanId : fa.planId;
  if (!planId) return null;
  const plan = plans.find((p) => p.id === planId);
  if (!plan) return null;
  const focus =
    fa.planId === planId
      ? fa
      : { planId, sectionIds: [], taskIds: [], milestoneIds: [] };
  return { plan, paneId: plan.owner.pane_id, ownerLabel: ownerLabelOf(plan), focus };
}

function excerpt(text: string, max = 200): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

// Auto-attached context: the in-view plan/task/milestone/section text so the
// orchestrator knows which region the message is reacting to.
function buildContext(plan: Plan, focus: FocusResult): string {
  const lines: string[] = [`[Canvas context — plan "${plan.title}"]`];

  const taskById = new Map(plan.tasks.map((t) => [t.id, t] as const));
  const inTasks = focus.taskIds.map((id) => taskById.get(id)).filter(Boolean) as Task[];
  if (inTasks.length) {
    lines.push("Tasks in view:");
    for (const t of inTasks) lines.push(`- ${t.name} (${t.status})${t.body ? `: ${excerpt(t.body)}` : ""}`);
  }

  const msById = new Map<string, Milestone>();
  for (const t of plan.tasks) for (const m of t.milestones) msById.set(m.id, m);
  const inMs = focus.milestoneIds.map((id) => msById.get(id)).filter(Boolean) as Milestone[];
  if (inMs.length) {
    lines.push("Milestones in view:");
    for (const m of inMs) lines.push(`- ${m.kind}: ${excerpt(m.text)}`);
  }

  const secById = new Map<string, Section>();
  collectSections(plan.sections, secById);
  const inSecs = focus.sectionIds.map((id) => secById.get(id)).filter(Boolean) as Section[];
  if (inSecs.length) {
    lines.push("Sections in view:");
    for (const s of inSecs) lines.push(`- ${s.title}${s.body ? `: ${excerpt(s.body)}` : ""}`);
  }

  return lines.join("\n");
}

async function sendToPane(api: ApiConfig, paneId: string, text: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/panes/${encodeURIComponent(paneId)}/send`, {
    method: "POST",
    headers: jsonHeaders(api),
    body: JSON.stringify({ text, enter: true }),
  });
  if (!res.ok) throw new Error(`send failed: ${res.status} ${await res.text()}`);
}

// Floating, context-addressed composer (§6): type on the canvas and the message
// routes to the orchestrator that owns the region under the viewport center.
export function CanvasComposer({
  api,
  plans,
  layout,
  viewportCenter,
  mode,
  settings = DEFAULT_CANVAS_SETTINGS,
}: CanvasComposerProps) {
  const focusTarget = useCanvasFocusTarget();
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const target = useMemo(
    () => resolveTarget(mode, plans, layout, viewportCenter, settings, focusTarget?.planId ?? null),
    [mode, plans, layout, viewportCenter, settings, focusTarget],
  );

  const canSend = !!target?.paneId && text.trim().length > 0 && !sending;

  const send = useCallback(async () => {
    const paneId = target?.paneId;
    if (!paneId || !target || !text.trim()) return;
    setSending(true);
    setError(null);
    const message = `${text.trim()}\n\n${buildContext(target.plan, target.focus)}`;
    try {
      await sendToPane(api, paneId, message);
      setText("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSending(false);
    }
  }, [api, target, text]);

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        if (canSend) void send();
      }
    },
    [canSend, send],
  );

  const hint = !target
    ? "Point the canvas at a plan to talk to its orchestrator"
    : !target.paneId
      ? `Plan "${target.plan.title}" has no orchestrator to talk to`
      : null;

  return (
    <div
      data-testid="canvas-composer"
      className="absolute bottom-4 left-1/2 z-30 flex w-[min(640px,90%)] -translate-x-1/2 flex-col gap-1.5 rounded-xl border p-2 shadow-lg"
      style={{
        background: "var(--ct-bg)",
        borderColor: "var(--ct-border)",
        color: "var(--ct-fg)",
      }}
      onMouseDown={(e) => e.stopPropagation()}
      onWheel={(e) => e.stopPropagation()}
    >
      <div className="flex items-center justify-between px-1 text-xs">
        {target ? (
          <span
            data-testid="canvas-composer-target"
            className="truncate"
            style={{ opacity: "var(--ct-text-secondary-opacity)" }}
            title={`Sending to ${target.ownerLabel}`}
          >
            → {target.plan.title} · {target.ownerLabel}
          </span>
        ) : (
          <span data-testid="canvas-composer-target" style={{ opacity: "var(--ct-text-secondary-opacity)" }}>
            → no target
          </span>
        )}
      </div>
      <div className="flex items-end gap-2">
        <textarea
          data-testid="canvas-composer-input"
          value={text}
          onInput={(e) => setText((e.target as HTMLTextAreaElement).value)}
          onKeyDown={onKeyDown}
          rows={1}
          placeholder={hint ?? `Message ${target?.ownerLabel ?? "orchestrator"}…`}
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
          style={{
            background: "var(--ct-accent, var(--ct-titlebar-bg))",
            borderColor: "var(--ct-border)",
            color: "var(--ct-fg)",
            cursor: canSend ? "pointer" : "not-allowed",
          }}
        >
          <Send size={14} />
        </button>
      </div>
      {error && (
        <div className="px-1 text-xs" style={{ color: "var(--ct-error, #f87171)" }}>
          {error}
        </div>
      )}
    </div>
  );
}

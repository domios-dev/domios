import { markPane } from "@codeterm/chat-core";
import { boxContains, type Point, type WorkspaceLayout } from "./layout";
import { buildSpatialIndex, type IndexedNode, type SpatialIndex } from "./canvasSpatialIndex";
import type { Plan, Task } from "./types";

// ONE primary referent for the composer: the open detail node wins, else the scene node
// nearest the viewport centre (task > section > plan by proximity). planId is ALWAYS set
// so routing resolves the plan owner. The id makes the message addressable, not prose.

export type CanvasContextKind = "spec" | "plan" | "section" | "task";

export interface CanvasContext {
  kind: CanvasContextKind;
  id: string;
  planId: string;
  title: string;
  excerpt?: string;
}

export interface OverlayRef {
  id: string;
  kind: string;
  planId: string;
}

function excerptOf(text: string | null | undefined, max = 180): string | undefined {
  if (!text) return undefined;
  const flat = text.replace(/\s+/g, " ").trim();
  if (!flat) return undefined;
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

function planContext(plan: Plan): CanvasContext {
  return { kind: "plan", id: plan.id, planId: plan.id, title: plan.title, excerpt: excerptOf(plan.goal?.vector) };
}

function taskContext(task: Task, plan: Plan): CanvasContext {
  return { kind: "task", id: task.id, planId: plan.id, title: task.name, excerpt: excerptOf(task.body) };
}

// A detail-overlay node (spec/plan/task) → its referent. Unknown kinds (handoff/ghost)
// degrade to the owning plan so the message still routes + addresses something real.
function fromOverlay(ref: OverlayRef, plan: Plan): CanvasContext {
  if (ref.kind === "task") {
    const task = plan.tasks.find((t) => t.id === ref.id);
    return task ? taskContext(task, plan) : planContext(plan);
  }
  if (ref.kind === "spec") return { kind: "spec", id: ref.id, planId: plan.id, title: `${plan.title} · spec`, excerpt: excerptOf(plan.goal?.vector) };
  return planContext(plan);
}

// Build the raycast index over every laid-out node box. Cheap; do it on layout change
// (memoize in the composer), NOT per paint.
export function buildCanvasNodeIndex(layout: WorkspaceLayout): SpatialIndex {
  const nodes: IndexedNode[] = [];
  for (const frame of layout.frames) for (const n of frame.nodes) nodes.push({ id: n.id, kind: n.kind, planId: n.planId, box: n.box });
  return buildSpatialIndex(nodes);
}

function nodeContext(node: IndexedNode, plan: Plan): CanvasContext | null {
  if (node.kind === "task") {
    const task = plan.tasks.find((t) => t.id === node.id);
    return task ? taskContext(task, plan) : null;
  }
  if (node.kind === "spec") return { kind: "spec", id: node.id, planId: plan.id, title: `${plan.title} · spec`, excerpt: excerptOf(plan.goal?.vector) };
  if (node.kind === "plan") return planContext(plan);
  return null; // handoff/ghost: not a chat referent — fall through to the plan region
}

export interface ResolveContextInput {
  overlay?: OverlayRef | null;
  plans: Plan[];
  layout: WorkspaceLayout;
  center: Point;
  index?: SpatialIndex;
}

// Spatial RAYCAST: the referent is the node whose world-box CONTAINS the viewport centre
// (smallest if nested) — exact, not proximity. Overlay-open node wins. If the point lands
// in empty space, fall back to the PLAN whose frame contains it (so the owner still
// switches by region). Resolution runs on camera-settle, off the frame path.
export function resolveCanvasContext(input: ResolveContextInput): CanvasContext | null {
  const { overlay, plans, layout, center } = input;
  const planById = new Map(plans.map((p) => [p.id, p] as const));

  // (a) overlay open → that exact node wins.
  if (overlay) {
    const plan = planById.get(overlay.planId);
    if (plan) return fromOverlay(overlay, plan);
  }

  // (b) raycast the centre point against node boxes (smallest containing).
  const index = input.index ?? buildCanvasNodeIndex(layout);
  const hit = index.queryPoint(center);
  if (hit) {
    const plan = planById.get(hit.planId);
    if (plan) {
      const ctx = nodeContext(hit, plan);
      if (ctx) return ctx;
    }
  }

  // (c) empty space → the plan whose frame region contains the centre → its owner.
  for (const frame of layout.frames) {
    if (boxContains(frame.box, center)) {
      const plan = planById.get(frame.planId);
      if (plan) return planContext(plan);
    }
  }
  return null;
}

const UNIT_LABEL: Record<CanvasContextKind, string> = { spec: "Spec", plan: "Plan", section: "Section", task: "Task" };

export function contextUnitLabel(kind: CanvasContextKind): string {
  return UNIT_LABEL[kind];
}

// "📍 Section · Foundation · sec_xxx" — the visible chip text (referent + real id).
export function contextChipLabel(ctx: CanvasContext): string {
  return `📍 ${UNIT_LABEL[ctx.kind]} · ${ctx.title} · ${ctx.id}`;
}

// The parseable reference prepended to the message: plan id + title always, plus the
// specific unit when it isn't the plan itself. The orchestrator resolves ids deterministically.
export function contextReference(ctx: CanvasContext, planTitle: string = ctx.title): string {
  const planRef = `plan ${ctx.planId} "${planTitle}"`;
  if (ctx.kind === "plan") return `[Canvas context → ${planRef}]`;
  return `[Canvas context → ${planRef}; ${ctx.kind} ${ctx.id} "${ctx.title}"]`;
}

// The outgoing message, wrapped on the existing pane marker envelope so the orchestrator's
// chat renders a provenance chip AND the agent reads explicit ids + a short excerpt.
export function buildCanvasMessage(text: string, ctx: CanvasContext, planTitle: string = ctx.title): string {
  const ref = contextReference(ctx, planTitle);
  const body = [text.trim(), ref, ctx.excerpt ? `> ${ctx.excerpt}` : ""].filter(Boolean).join("\n\n");
  return markPane(`📍 Canvas · ${UNIT_LABEL[ctx.kind]} · ${ctx.title}`, ctx.planId, body);
}

// Override the referent down to plan-level (the chip's click action).
export function toPlanContext(ctx: CanvasContext, plans: Plan[]): CanvasContext | null {
  const plan = plans.find((p) => p.id === ctx.planId);
  return plan ? planContext(plan) : null;
}

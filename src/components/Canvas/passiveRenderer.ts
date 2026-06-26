import { TASK_STATUS_COLORS } from "./settings";
import type { CanvasScene, ProjectedSceneBox } from "./scene";
import { projectSceneBoxes, projectWorldBox, projectWorldPoint } from "./scene";
import type { PlanStatus, TaskStatus } from "./types";
import type { ViewportTransform } from "./useCanvasViewport";

export type PassiveRingKind = "focus" | "dominant" | "blink" | "pulse" | "highlight" | "tag" | "hover";

export interface PassiveRenderFocus {
  focusedNodeId: string | null;
  dominantNodeId: string | null;
  hoveredTask: string | null;
  highlightTaskIds: ReadonlySet<string> | null;
  tagFilterMatchIds: ReadonlySet<string> | null;
  blinkIds: ReadonlySet<string>;
  pulsePlanIds: ReadonlySet<string>;
}

export interface PassiveRendererPalette {
  bg: string;
  fg: string;
  border: string;
  borderFocus: string;
  groupStroke: string;
  taskStatus: Record<TaskStatus, string>;
  planActive: string;
  planArchived: string;
}

export interface PassivePixelSample {
  width: number;
  height: number;
  filledRatio: number;
  nonblank: boolean;
  missing: boolean;
}

export interface PassiveCanvasDebugRegistry {
  getCanvas: () => HTMLCanvasElement | null;
  samplePixels: (alphaThreshold?: number) => PassivePixelSample;
  projectedBoxes: () => ProjectedSceneBox[];
}

const RING_ORDER: PassiveRingKind[] = ["focus", "dominant", "blink", "pulse", "highlight", "tag", "hover"];

const RING_STROKE: Record<PassiveRingKind, { color: string; width: number }> = {
  focus: { color: "rgba(129, 140, 248, 0.95)", width: 3 },
  dominant: { color: "rgba(129, 140, 248, 0.75)", width: 2.5 },
  blink: { color: "rgba(129, 140, 248, 0.9)", width: 2.5 },
  pulse: { color: "rgba(129, 140, 248, 0.65)", width: 2 },
  highlight: { color: "rgba(245, 158, 11, 0.9)", width: 2 },
  tag: { color: "rgba(129, 140, 248, 0.85)", width: 2 },
  hover: { color: "rgba(148, 163, 184, 0.85)", width: 1.5 },
};

declare global {
  interface Window {
    __codetermCanvasPassive?: PassiveCanvasDebugRegistry;
  }
}

let debugCanvas: HTMLCanvasElement | null = null;
let debugScene: CanvasScene | null = null;
let debugTransform: ViewportTransform | null = null;

function readCssColor(host: HTMLElement, token: string, fallback: string): string {
  if (!token.startsWith("var(")) return token || fallback;
  const match = token.match(/var\((--[^,\s)]+)/);
  if (!match) return fallback;
  return host.ownerDocument.defaultView?.getComputedStyle(host).getPropertyValue(match[1]).trim() || fallback;
}

export function readPassivePalette(host: HTMLElement): PassiveRendererPalette {
  return {
    bg: readCssColor(host, "var(--ct-bg)", "#111318"),
    fg: readCssColor(host, "var(--ct-fg)", "#e8eaed"),
    border: readCssColor(host, "var(--ct-border-default)", "rgba(148, 163, 184, 0.45)"),
    borderFocus: readCssColor(host, "var(--ct-border-focus)", "rgba(129, 140, 248, 0.92)"),
    groupStroke: readCssColor(host, "var(--ct-border-subtle)", "rgba(148, 163, 184, 0.35)"),
    taskStatus: { ...TASK_STATUS_COLORS },
    planActive: readCssColor(host, "var(--ct-border-focus)", "rgba(129, 140, 248, 0.92)"),
    planArchived: readCssColor(host, "rgba(100, 116, 139, 0.9)", "rgba(100, 116, 139, 0.9)"),
  };
}

export function statusColorForNode(
  kind: string,
  status: TaskStatus | PlanStatus | null,
  palette: PassiveRendererPalette,
): string {
  if (kind === "task" && status) return palette.taskStatus[status as TaskStatus] ?? palette.border;
  if (kind === "plan") return status === "archived" ? palette.planArchived : palette.planActive;
  return palette.borderFocus;
}

export function ringKindForNode(id: string, focus: PassiveRenderFocus): PassiveRingKind | null {
  if (focus.focusedNodeId === id) return "focus";
  if (focus.dominantNodeId === id) return "dominant";
  if (focus.blinkIds.has(id)) return "blink";
  if (focus.pulsePlanIds.has(id)) return "pulse";
  if (focus.highlightTaskIds?.has(id)) return "highlight";
  if (focus.tagFilterMatchIds?.has(id)) return "tag";
  if (focus.hoveredTask === id) return "hover";
  return null;
}

export function truncateLabel(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string {
  if (maxWidth <= 4) return "";
  if (ctx.measureText(text).width <= maxWidth) return text;
  const ell = "…";
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (ctx.measureText(text.slice(0, mid) + ell).width <= maxWidth) lo = mid;
    else hi = mid - 1;
  }
  return lo > 0 ? text.slice(0, lo) + ell : ell;
}

function fillWithAlpha(color: string, alpha: number): string {
  const match = color.match(/rgba?\(([^)]+)\)/);
  if (!match) return color;
  const parts = match[1].split(",").map((p) => p.trim());
  if (parts.length === 3) return `rgba(${parts[0]}, ${parts[1]}, ${parts[2]}, ${alpha})`;
  if (parts.length === 4) return `rgba(${parts[0]}, ${parts[1]}, ${parts[2]}, ${alpha})`;
  return color;
}

function drawRoundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  const radius = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + w, y, x + w, y + h, radius);
  ctx.arcTo(x + w, y + h, x, y + h, radius);
  ctx.arcTo(x, y + h, x, y, radius);
  ctx.arcTo(x, y, x + w, y, radius);
  ctx.closePath();
}

function drawNode(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  label: string,
  accent: string,
  palette: PassiveRendererPalette,
  ring: PassiveRingKind | null,
): void {
  if (w < 2 || h < 2) return;
  const radius = Math.min(8, w / 4, h / 4);
  drawRoundedRect(ctx, x + 0.5, y + 0.5, w - 1, h - 1, radius);
  ctx.fillStyle = fillWithAlpha(palette.bg, 0.72);
  ctx.fill();
  ctx.strokeStyle = accent;
  ctx.lineWidth = 1.25;
  ctx.stroke();

  if (w >= 42 && h >= 18) {
    const pad = Math.max(4, Math.min(8, w * 0.08));
    const fontSize = Math.max(9, Math.min(12, h * 0.22));
    ctx.font = `600 ${fontSize}px var(--ct-font-ui, system-ui, sans-serif)`;
    ctx.fillStyle = palette.fg;
    ctx.textBaseline = "top";
    const clipped = truncateLabel(ctx, label, Math.max(0, w - pad * 2));
    if (clipped) ctx.fillText(clipped, x + pad, y + pad);
  }

  if (ring) {
    const spec = RING_STROKE[ring];
    drawRoundedRect(ctx, x + 0.5, y + 0.5, w - 1, h - 1, radius);
    ctx.strokeStyle = spec.color;
    ctx.lineWidth = spec.width;
    ctx.stroke();
  }
}

function drawEdge(
  ctx: CanvasRenderingContext2D,
  fromX: number,
  fromY: number,
  toX: number,
  toY: number,
  palette: PassiveRendererPalette,
  dimmed: boolean,
): void {
  ctx.beginPath();
  ctx.moveTo(fromX, fromY);
  ctx.lineTo(toX, toY);
  ctx.strokeStyle = palette.borderFocus;
  ctx.lineWidth = dimmed ? 1 : 1.6;
  ctx.globalAlpha = dimmed ? 0.14 : 0.55;
  ctx.stroke();
  ctx.globalAlpha = 1;
}

function edgeDimmed(
  edgeTo: string | null,
  focus: PassiveRenderFocus,
  tagFilterActive: boolean,
): boolean {
  const related =
    (focus.hoveredTask != null && edgeTo === focus.hoveredTask) ||
    (focus.highlightTaskIds != null && edgeTo != null && focus.highlightTaskIds.has(edgeTo)) ||
    (tagFilterActive && edgeTo != null && focus.tagFilterMatchIds?.has(edgeTo));
  const anyFocus =
    focus.hoveredTask != null || focus.highlightTaskIds != null || tagFilterActive;
  return anyFocus && !related;
}

function screenBoxVisible(
  box: { x: number; y: number; width: number; height: number },
  viewportWidth: number,
  viewportHeight: number,
  pad = 80,
): boolean {
  return (
    box.x + box.width >= -pad &&
    box.y + box.height >= -pad &&
    box.x <= viewportWidth + pad &&
    box.y <= viewportHeight + pad
  );
}

function edgeScreenBox(from: { x: number; y: number }, to: { x: number; y: number }) {
  const x = Math.min(from.x, to.x);
  const y = Math.min(from.y, to.y);
  return { x, y, width: Math.abs(from.x - to.x), height: Math.abs(from.y - to.y) };
}

export function drawPassiveScene(
  ctx: CanvasRenderingContext2D,
  scene: CanvasScene,
  transform: ViewportTransform,
  viewportWidth: number,
  viewportHeight: number,
  palette: PassiveRendererPalette,
  focus: PassiveRenderFocus,
  tagFilterActive = false,
): void {
  ctx.clearRect(0, 0, viewportWidth, viewportHeight);
  ctx.save();

  for (const group of scene.groups) {
    const box = projectWorldBox(group.worldBox, transform);
    if (box.width < 2 || box.height < 2 || !screenBoxVisible(box, viewportWidth, viewportHeight)) continue;
    drawRoundedRect(ctx, box.x + 0.5, box.y + 0.5, box.width - 1, box.height - 1, 10);
    ctx.setLineDash([6, 4]);
    ctx.strokeStyle = palette.groupStroke;
    ctx.lineWidth = 1.25;
    ctx.stroke();
    ctx.setLineDash([]);
  }

  for (const edge of scene.edges) {
    const a = projectWorldPoint(edge.anchors.from, transform);
    const b = projectWorldPoint(edge.anchors.to, transform);
    if (!screenBoxVisible(edgeScreenBox(a, b), viewportWidth, viewportHeight)) continue;
    drawEdge(ctx, a.x, a.y, b.x, b.y, palette, edgeDimmed(edge.to, focus, tagFilterActive));
  }

  for (const node of scene.nodes) {
    const box = projectWorldBox(node.worldBox, transform);
    if (!screenBoxVisible(box, viewportWidth, viewportHeight)) continue;
    const accent = statusColorForNode(node.kind, node.status, palette);
    const ring = ringKindForNode(node.id, focus);
    drawNode(ctx, box.x, box.y, box.width, box.height, node.semanticText, accent, palette, ring);
  }

  ctx.restore();
}

export function samplePassiveCanvasPixels(
  canvas: HTMLCanvasElement | null,
  alphaThreshold = 8,
): PassivePixelSample {
  if (!canvas || canvas.tagName !== "CANVAS") {
    return { width: 0, height: 0, filledRatio: 0, nonblank: false, missing: true };
  }
  const width = canvas.width;
  const height = canvas.height;
  if (width < 1 || height < 1) {
    return { width, height, filledRatio: 0, nonblank: false, missing: false };
  }
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return { width, height, filledRatio: 0, nonblank: false, missing: false };
  const { data } = ctx.getImageData(0, 0, width, height);
  let filled = 0;
  const pixels = width * height;
  for (let i = 3; i < data.length; i += 4) {
    if (data[i] > alphaThreshold) filled += 1;
  }
  const filledRatio = filled / pixels;
  return { width, height, filledRatio, nonblank: filled > 0, missing: false };
}

export function publishPassiveCanvasDebug(
  canvas: HTMLCanvasElement | null,
  scene: CanvasScene,
  transform: ViewportTransform,
): void {
  if (typeof window === "undefined") return;
  debugCanvas = canvas;
  debugScene = scene;
  debugTransform = transform;
  window.__codetermCanvasPassive = {
    getCanvas: () => debugCanvas,
    samplePixels: (alphaThreshold) => samplePassiveCanvasPixels(debugCanvas, alphaThreshold),
    projectedBoxes: () => (debugScene && debugTransform ? projectSceneBoxes(debugScene, debugTransform) : []),
  };
}

export function passiveRingPriority(): readonly PassiveRingKind[] {
  return RING_ORDER;
}

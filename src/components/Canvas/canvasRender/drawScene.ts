
import { resolveEdgeEnds } from "../edges";
import type { LaidOutEdge, LaidOutNode, Point } from "../layout";
import { projectWorldBox } from "../scene";
import type { ViewportTransform } from "../useCanvasViewport";
import { buildCardContent, pickLevel } from "./cardContent";
import type { HitRegion, RegionAction } from "./drawCard";
import { drawLodMarker, hitTestLodMarker } from "./drawLodMarker";
import type { NodeBitmapCache } from "./nodeBitmapCache";
import type { CanvasPalette } from "./palette";
import { PRESENCE_INK, agentInitials, type PresenceAgent } from "../presence/identity";
import type { ResolvedSceneLod, SectionTaskGroup } from "./sceneLod";
import { markerToSyntheticNode, resolveSceneLod } from "./sceneLod";
import type { Task } from "../types";

const FONT = "Inter, -apple-system, BlinkMacSystemFont, system-ui, sans-serif";
const CULL_MARGIN = 280;

export interface ZoneLabel {
  text: string;
  anchor: Point;
}

export interface SectionZoneDraw {
  hull: Point[];
  color: string;
  box?: { x: number; y: number; width: number; height: number };
}

// An orchestrator avatar floating free + its dashed connectors to each owned plan node.
export interface GroupGlyph {
  ownerKey: string;
  label: string;
  live: boolean;
  anchor: Point;
  planAnchors: Point[];
}

export interface SceneDrawInput {
  nodes: readonly LaidOutNode[];
  edges: readonly LaidOutEdge[];
  nodeById: Map<string, LaidOutNode>;
  zoneLabels: readonly ZoneLabel[];
  sectionZones?: readonly SectionZoneDraw[];
  groups: readonly GroupGlyph[];
  /** taskId → live agents executing it (drawn as small initials chips on the card). */
  presenceByNode: ReadonlyMap<string, PresenceAgent[]>;
  /** The node's model (for the readback + painter dispatch). */
  resolveModel: (node: LaidOutNode) => unknown;
  /** Cache key for the node's painter; null = nothing to draw (skip). Memoized upstream. */
  signatureOf: (node: LaidOutNode) => string | null;
  /** Paint a node to ctx at logical w×h via its registered canvas painter → hit regions. */
  paintNode: (ctx: CanvasRenderingContext2D, node: LaidOutNode, w: number, h: number) => HitRegion[];
  cache: NodeBitmapCache;
  palette: CanvasPalette;
  dpr: number;
  width: number;
  height: number;
  /** Camera-independent section rollups for overview / done-stack LOD. */
  lodGroups?: readonly SectionTaskGroup[];
  taskById?: Map<string, Task>;
}

// a hover/highlight never invalidates a bitmap; it's painted as a primitive overlay.
export interface SceneExtras {
  hoveredId?: string | null;
  dimIds?: ReadonlySet<string> | null;
  highlightIds?: ReadonlySet<string> | null;
  blinkIds?: ReadonlySet<string> | null;
  dominantId?: string | null;
}

function regionRank(a: RegionAction): number {
  if (a.type === "section-button") return 4;
  if (a.type === "expand") return 3;
  if (a.type === "section-cluster" || a.type === "done-stack") return 3;
  if (a.type === "section") return 2;
  return 0;
}

function resolveLod(input: SceneDrawInput, t: ViewportTransform): ResolvedSceneLod | null {
  if (!input.lodGroups?.length || !input.taskById) return null;
  return resolveSceneLod(t.scale, input.lodGroups, input.taskById);
}

// the highest-rank region inside it). Drives clicks + hover without any DOM hit targets.
export function hitTestScene(input: SceneDrawInput, t: ViewportTransform, point: Point): { node: LaidOutNode; action: RegionAction } | null {
  const { nodes, cache } = input;
  const lod = resolveLod(input, t);
  const hidden = lod?.hiddenNodeIds ?? null;
  const lodHit = lod?.markers.length ? hitTestLodMarker(lod.markers, t, point) : null;
  let best: { node: LaidOutNode; sx: number; sy: number; sw: number; sh: number } | null = null;
  for (const node of nodes) {
    if (hidden?.has(node.id)) continue;
    const sb = projectWorldBox(node.box, t);
    if (point.x < sb.x || point.x > sb.x + sb.width || point.y < sb.y || point.y > sb.y + sb.height) continue;
    const area = sb.width * sb.height;
    if (!best || area < best.sw * best.sh) best = { node, sx: sb.x, sy: sb.y, sw: sb.width, sh: sb.height };
  }
  if (lodHit) {
    const marker = lod!.markers.find((m) => m.id === lodHit.markerId) ?? lod!.markers[0];
    const markerNode = { ...markerToSyntheticNode(marker), iteration: null, manual: false } as LaidOutNode;
    const markerArea = lodHit.box.width * lodHit.box.height;
    if (!best || markerArea < best.sw * best.sh) {
      return { node: markerNode, action: lodHit.action };
    }
  }
  if (!best) return null;
  let action: RegionAction = { type: "node" };
  const bmp = cache.get(best.node.id);
  if (bmp && best.sw > 0 && best.sh > 0) {
    const lx = (point.x - best.sx) * (bmp.w / best.sw);
    const ly = (point.y - best.sy) * (bmp.h / best.sh);
    let rank = -1;
    for (const r of bmp.regions) {
      if (lx < r.x || lx > r.x + r.w || ly < r.y || ly > r.y + r.h) continue;
      const rr = regionRank(r.action);
      if (rr > rank) { rank = rr; action = r.action; }
    }
  }
  return { node: best.node, action };
}

function project(p: Point, t: ViewportTransform): Point {
  return { x: p.x * t.scale + t.x, y: p.y * t.scale + t.y };
}

function offscreen(a: Point, b: Point, w: number, h: number): boolean {
  const minX = Math.min(a.x, b.x);
  const maxX = Math.max(a.x, b.x);
  const minY = Math.min(a.y, b.y);
  const maxY = Math.max(a.y, b.y);
  return maxX < -CULL_MARGIN || minX > w + CULL_MARGIN || maxY < -CULL_MARGIN || minY > h + CULL_MARGIN;
}

function drawEdge(ctx: CanvasRenderingContext2D, a: Point, b: Point): void {
  const dx = b.x - a.x;
  const hb = Math.max(Math.abs(dx) * 0.5, 24);
  const c1x = a.x + (dx >= 0 ? hb : -hb);
  const c2x = b.x + (dx >= 0 ? -hb : hb);
  ctx.beginPath();
  ctx.moveTo(a.x, a.y);
  ctx.bezierCurveTo(c1x, a.y, c2x, b.y, b.x, b.y);
  ctx.stroke();
}

// a gesture settles (the blit loop scale-blits stale bitmaps during the gesture).
export function resharpenVisible(input: SceneDrawInput, t: ViewportTransform): Set<string> {
  const { nodes, signatureOf, paintNode, cache, dpr, width, height } = input;
  const hidden = resolveLod(input, t)?.hiddenNodeIds ?? null;
  const keep = new Set<string>();
  for (const node of nodes) {
    if (hidden?.has(node.id)) continue;
    const sb = projectWorldBox(node.box, t);
    if (sb.x + sb.width < -CULL_MARGIN || sb.x > width + CULL_MARGIN || sb.y + sb.height < -CULL_MARGIN || sb.y > height + CULL_MARGIN) continue;
    const sig = signatureOf(node);
    if (sig == null) continue;
    keep.add(node.id);
    cache.resharpen(node.id, sig, sb.width, sb.height, dpr, (ctx) => paintNode(ctx, node, sb.width, sb.height));
  }
  cache.prune(keep);
  return keep;
}

// (the cards are pixels, not queryable elements). Published to window for specs.
export interface RenderReadbackNode {
  id: string;
  kind: string;
  level: "far" | "near";
  kicker: string;
  title: string;
  tags: string[];
  sectionTitles: string[];
  fields: string[];
}

export function buildReadback(input: SceneDrawInput, t: ViewportTransform): RenderReadbackNode[] {
  const { nodes, resolveModel, width, height } = input;
  const hidden = resolveLod(input, t)?.hiddenNodeIds ?? null;
  const cull = width > 0 && height > 0;
  const out: RenderReadbackNode[] = [];
  for (const node of nodes) {
    if (hidden?.has(node.id)) continue;
    const sb = projectWorldBox(node.box, t);
    if (cull && (sb.x + sb.width < -CULL_MARGIN || sb.x > width + CULL_MARGIN || sb.y + sb.height < -CULL_MARGIN || sb.y > height + CULL_MARGIN)) continue;
    const content = buildCardContent(node.kind, resolveModel(node));
    if (!content) continue;
    out.push({
      id: node.id,
      kind: node.kind,
      level: pickLevel(content, sb.width, sb.height),
      kicker: content.kicker,
      title: content.title,
      tags: content.tags.map((tg) => tg.text),
      sectionTitles: content.sections.map((s) => s.title),
      fields: content.fields.map((f) => `${f.label}:${f.value}`),
    });
  }
  return out;
}

function strokeRing(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, color: string, lw: number): void {
  const r = Math.min(13, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
  ctx.strokeStyle = color;
  ctx.lineWidth = lw;
  ctx.stroke();
}

function drawPresence(ctx: CanvasRenderingContext2D, agents: PresenceAgent[], x: number, y: number): void {
  const rad = 8;
  let cx = x + rad;
  for (const a of agents.slice(0, 4)) {
    ctx.beginPath();
    ctx.arc(cx, y, rad, 0, Math.PI * 2);
    ctx.fillStyle = a.color;
    ctx.fill();
    ctx.fillStyle = PRESENCE_INK;
    ctx.font = `700 8px ${FONT}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(agentInitials(a.label), cx, y + 0.5);
    cx += rad * 1.7;
  }
  ctx.textAlign = "left";
}

export function drawScene(ctx: CanvasRenderingContext2D, input: SceneDrawInput, t: ViewportTransform, extras: SceneExtras = {}): void {
  const { nodes, edges, nodeById, zoneLabels, groups, presenceByNode, signatureOf, paintNode, cache, palette, dpr, width, height } = input;
  const { hoveredId, dimIds, highlightIds, blinkIds, dominantId } = extras;
  const lod = resolveLod(input, t);
  const hidden = lod?.hiddenNodeIds ?? null;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);

  // Orchestrator → plan connectors (dashed, under the cards).
  if (groups.length) {
    ctx.save();
    ctx.setLineDash([5, 6]);
    ctx.lineWidth = 1.5;
    for (const g of groups) {
      const a = project(g.anchor, t);
      ctx.strokeStyle = g.live ? "rgba(56,189,248,0.6)" : palette.border;
      for (const pa of g.planAnchors) {
        const b = project(pa, t);
        if (offscreen(a, b, width, height)) continue;
        const my = a.y + (b.y - a.y) * 0.5;
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.bezierCurveTo(a.x, my, b.x, my, b.x, b.y);
        ctx.stroke();
      }
    }
    ctx.restore();
  }

  // v3 zone containers (rounded rects under cards).
  if (input.sectionZones?.length) {
    ctx.save();
    for (const z of input.sectionZones) {
      const box = z.box;
      if (!box) continue;
      const sb = projectWorldBox(box, t);
      if (sb.x + sb.width < -CULL_MARGIN || sb.x > width + CULL_MARGIN || sb.y + sb.height < -CULL_MARGIN || sb.y > height + CULL_MARGIN) continue;
      const r = Math.min(16, sb.width / 2, sb.height / 2);
      ctx.beginPath();
      ctx.moveTo(sb.x + r, sb.y);
      ctx.arcTo(sb.x + sb.width, sb.y, sb.x + sb.width, sb.y + sb.height, r);
      ctx.arcTo(sb.x + sb.width, sb.y + sb.height, sb.x, sb.y + sb.height, r);
      ctx.arcTo(sb.x, sb.y + sb.height, sb.x, sb.y, r);
      ctx.arcTo(sb.x, sb.y, sb.x + sb.width, sb.y, r);
      ctx.closePath();
      ctx.fillStyle = z.color;
      ctx.globalAlpha = 0.12;
      ctx.fill();
      ctx.strokeStyle = z.color;
      ctx.globalAlpha = 0.55;
      ctx.lineWidth = 1.5;
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
    ctx.restore();
  }

  // highlighted node light up — brighter (alpha) AND thicker so they clearly stand out.
  ctx.save();
  ctx.lineCap = "round";
  for (const edge of edges) {
    const ends = resolveEdgeEnds(edge, nodeById);
    if (!ends) continue;
    const a = project(ends.a, t);
    const b = project(ends.b, t);
    if (offscreen(a, b, width, height)) continue;
    const lit = highlightIds && ((edge.to && highlightIds.has(edge.to)) || (edge.from && highlightIds.has(edge.from)));
    ctx.strokeStyle = lit ? palette.accent : palette.borderStrong;
    ctx.globalAlpha = lit ? 0.95 : 0.3;
    ctx.lineWidth = lit ? 2.2 : 1.2;
    drawEdge(ctx, a, b);
  }
  ctx.restore();

  // Zone name labels.
  if (zoneLabels.length) {
    ctx.save();
    ctx.font = `600 12px ${FONT}`;
    ctx.fillStyle = palette.fgMuted;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.globalAlpha = 0.7;
    for (const z of zoneLabels) {
      const p = project(z.anchor, t);
      if (p.x < -CULL_MARGIN || p.x > width + CULL_MARGIN || p.y < -CULL_MARGIN || p.y > height + CULL_MARGIN) continue;
      ctx.fillText(z.text, p.x, p.y);
    }
    ctx.restore();
  }

  // Nodes: blit cached bitmaps + emphasis rings + on-card presence.
  const tagDimActive = dimIds != null;
  for (const node of nodes) {
    if (hidden?.has(node.id)) continue;
    const sb = projectWorldBox(node.box, t);
    if (sb.x + sb.width < -CULL_MARGIN || sb.x > width + CULL_MARGIN || sb.y + sb.height < -CULL_MARGIN || sb.y > height + CULL_MARGIN) continue;
    if (sb.width < 2 || sb.height < 2) continue;
    const sig = signatureOf(node);
    if (sig == null) continue;
    const bmp = cache.ensure(node.id, sig, sb.width, sb.height, dpr, (ctx) => paintNode(ctx, node, sb.width, sb.height));
    const dimmed = tagDimActive && node.kind === "task" && !dimIds!.has(node.id);
    ctx.globalAlpha = dimmed ? 0.18 : 1;
    ctx.drawImage(bmp.canvas, sb.x, sb.y, sb.width, sb.height);
    ctx.globalAlpha = 1;

    if (node.id === dominantId) strokeRing(ctx, sb.x + 0.5, sb.y + 0.5, sb.width - 1, sb.height - 1, palette.accent, 2.5);
    if (blinkIds?.has(node.id)) strokeRing(ctx, sb.x - 1.5, sb.y - 1.5, sb.width + 3, sb.height + 3, palette.borderStrong, 3);
    else if (highlightIds?.has(node.id)) strokeRing(ctx, sb.x, sb.y, sb.width, sb.height, palette.borderStrong, 2.5);
    else if (node.id === hoveredId) strokeRing(ctx, sb.x, sb.y, sb.width, sb.height, palette.borderStrong, 1.6);

    if (sb.width > 64 && sb.height > 40) {
      const agents = presenceByNode.get(node.id);
      if (agents && agents.length) drawPresence(ctx, agents, sb.x + 10, sb.y + sb.height - 12);
    }
  }

  if (lod?.markers.length) {
    for (const marker of lod.markers) {
      const sb = projectWorldBox(marker.box, t);
      if (sb.x + sb.width < -CULL_MARGIN || sb.x > width + CULL_MARGIN || sb.y + sb.height < -CULL_MARGIN || sb.y > height + CULL_MARGIN) continue;
      drawLodMarker(ctx, marker, sb.x, sb.y, sb.width, sb.height, palette);
      const lit = marker.id === hoveredId || highlightIds?.has(marker.id);
      if (lit) strokeRing(ctx, sb.x, sb.y, sb.width, sb.height, palette.borderStrong, 2);
    }
  }

  // Orchestrator avatars (on top of connectors/cards).
  if (groups.length) {
    for (const g of groups) {
      const a = project(g.anchor, t);
      if (a.x < -CULL_MARGIN || a.x > width + CULL_MARGIN || a.y < -CULL_MARGIN || a.y > height + CULL_MARGIN) continue;
      ctx.beginPath();
      ctx.arc(a.x, a.y, 15, 0, Math.PI * 2);
      ctx.fillStyle = g.live ? palette.live : palette.cardBgElev;
      ctx.fill();
      ctx.strokeStyle = g.live ? palette.live : palette.border;
      ctx.lineWidth = 1.5;
      ctx.stroke();
      ctx.fillStyle = g.live ? palette.bg : palette.fg;
      ctx.font = `700 11px ${FONT}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(agentInitials(g.label), a.x, a.y + 0.5);
      ctx.textAlign = "left";
    }
  }
}

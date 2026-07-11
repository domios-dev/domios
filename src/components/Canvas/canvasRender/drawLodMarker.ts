import type { Box } from "../layout";
import type { PresenceAgent } from "../presence/identity";
import { TASK_STATUS_COLORS } from "../settings";
import type { RegionAction } from "./drawCard";
import type { CanvasPalette } from "./palette";
import type { LodMarker, StatusCounts } from "./sceneLod";
import { agentInitials } from "../presence/identity";

const FONT = "Inter, -apple-system, BlinkMacSystemFont, system-ui, sans-serif";

export interface LodHitRegion {
  markerId: string;
  box: Box;
  action: RegionAction;
}

function roundRectPath(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number): void {
  const rr = Math.max(0, Math.min(r, w / 2, h / 2));
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}

function countLine(counts: StatusCounts): string {
  const parts: string[] = [];
  if (counts.in_progress) parts.push(`${counts.in_progress} active`);
  if (counts.in_review) parts.push(`${counts.in_review} review`);
  if (counts.blocked) parts.push(`${counts.blocked} blocked`);
  if (counts.paused) parts.push(`${counts.paused} paused`);
  if (counts.pending) parts.push(`${counts.pending} pending`);
  const done = counts.done + counts.cancelled;
  if (done) parts.push(`${done} done`);
  return parts.slice(0, 3).join(" · ") || "0 tasks";
}

function drawPresenceDots(ctx: CanvasRenderingContext2D, agents: PresenceAgent[], x: number, y: number, max: number): void {
  const rad = 5;
  let cx = x;
  for (const a of agents.slice(0, max)) {
    ctx.beginPath();
    ctx.arc(cx, y, rad, 0, Math.PI * 2);
    ctx.fillStyle = a.color;
    ctx.fill();
    ctx.fillStyle = "#fff";
    ctx.font = `700 6px ${FONT}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(agentInitials(a.label), cx, y + 0.5);
    cx += rad * 2.2;
  }
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
}

export function drawLodMarker(
  ctx: CanvasRenderingContext2D,
  marker: LodMarker,
  sx: number,
  sy: number,
  sw: number,
  sh: number,
  p: CanvasPalette,
): LodHitRegion {
  const pad = Math.max(6, Math.min(10, sw * 0.08));
  const accent = marker.kind === "done-stack" ? TASK_STATUS_COLORS.done : p.accent;

  roundRectPath(ctx, sx + 0.5, sy + 0.5, sw - 1, sh - 1, Math.min(10, sh / 3));
  ctx.fillStyle = p.cardBgElev;
  ctx.fill();
  ctx.strokeStyle = accent;
  ctx.lineWidth = marker.kind === "done-stack" ? 1.2 : 1.5;
  roundRectPath(ctx, sx + 0.5, sy + 0.5, sw - 1, sh - 1, Math.min(10, sh / 3));
  ctx.stroke();

  ctx.save();
  roundRectPath(ctx, sx + 1, sy + 1, sw - 2, sh - 2, Math.min(9, sh / 3));
  ctx.clip();

  if (marker.kind === "done-stack") {
    const n = marker.counts.done + marker.counts.cancelled;
    ctx.font = `700 ${Math.max(10, Math.min(14, sh * 0.42))}px ${FONT}`;
    ctx.fillStyle = TASK_STATUS_COLORS.done;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(`${n} done`, sx + sw / 2, sy + sh / 2 + 0.5);
    ctx.textAlign = "left";
    ctx.restore();
    return {
      markerId: marker.id,
      box: { x: sx, y: sy, width: sw, height: sh },
      action: { type: "done-stack", planId: marker.planId, sectionId: marker.sectionId },
    };
  }

  const titlePx = Math.max(10, Math.min(13, sh * 0.22));
  const subPx = Math.max(8, Math.min(10, sh * 0.17));
  ctx.font = `600 ${titlePx}px ${FONT}`;
  ctx.fillStyle = p.fg;
  const title = marker.title.length > 22 ? `${marker.title.slice(0, 21)}…` : marker.title;
  ctx.fillText(title, sx + pad, sy + pad + titlePx);

  ctx.font = `500 ${subPx}px ${FONT}`;
  ctx.fillStyle = p.fgMuted;
  ctx.fillText(countLine(marker.counts), sx + pad, sy + pad + titlePx + 4 + subPx);

  if (marker.presence.length && sh > 36) {
    drawPresenceDots(ctx, marker.presence, sx + pad, sy + sh - pad - 4, 4);
  }
  ctx.restore();

  return {
    markerId: marker.id,
    box: { x: sx, y: sy, width: sw, height: sh },
    action: { type: "section-cluster", planId: marker.planId, sectionId: marker.sectionId },
  };
}

export function hitTestLodMarker(markers: readonly LodMarker[], t: { scale: number; x: number; y: number }, point: { x: number; y: number }): LodHitRegion | null {
  let best: { region: LodHitRegion; area: number } | null = null;
  for (const m of markers) {
    const sx = m.box.x * t.scale + t.x;
    const sy = m.box.y * t.scale + t.y;
    const sw = m.box.width * t.scale;
    const sh = m.box.height * t.scale;
    if (point.x < sx || point.x > sx + sw || point.y < sy || point.y > sy + sh) continue;
    const area = sw * sh;
    const action: RegionAction =
      m.kind === "done-stack"
        ? { type: "done-stack", planId: m.planId, sectionId: m.sectionId }
        : { type: "section-cluster", planId: m.planId, sectionId: m.sectionId };
    const region: LodHitRegion = { markerId: m.id, box: { x: sx, y: sy, width: sw, height: sh }, action };
    if (!best || area < best.area) best = { region, area };
  }
  return best?.region ?? null;
}

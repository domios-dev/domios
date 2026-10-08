
import type { CardContent, CardLevel, CardTag } from "./cardContent";
import { FIT, GLYPH_ACCENT } from "./cardContent";
import type { CanvasPalette } from "./palette";
import { ellipsize, wrapLines } from "./textMetrics";
import { worldThresholds } from "../worlds";

const FONT = "Inter, -apple-system, BlinkMacSystemFont, system-ui, sans-serif";
const PAD = 12;
const TITLE_PX = 14;

export type RegionAction =
  | { type: "node" }
  | { type: "expand" }
  | { type: "section"; sectionId: string }
  | { type: "section-button"; sectionId: string }
  | { type: "section-cluster"; planId: string; sectionId: string }
  | { type: "done-stack"; planId: string; sectionId: string };

export interface HitRegion {
  x: number;
  y: number;
  w: number;
  h: number;
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

const measureWith = (ctx: CanvasRenderingContext2D) => (t: string) => ctx.measureText(t).width;

function drawChips(ctx: CanvasRenderingContext2D, tags: CardTag[], x: number, y: number, maxX: number, fontPx: number, p: CanvasPalette): void {
  ctx.font = `${fontPx}px ${FONT}`;
  ctx.textBaseline = "middle";
  const h = fontPx + 6;
  let cx = x;
  for (const tag of tags) {
    const w = ctx.measureText(tag.text).width + 12;
    if (cx + w > maxX) break;
    ctx.fillStyle = tag.color ?? p.chipBg;
    roundRectPath(ctx, cx, y, w, h, h / 2);
    ctx.fill();
    ctx.fillStyle = tag.color ? p.onAccent : p.fg;
    ctx.fillText(tag.text, cx + 6, y + h / 2 + 0.5);
    cx += w + 5;
  }
  ctx.textBaseline = "alphabetic";
}

// A clear expand affordance; avoid status-like wording such as "Open".
function drawViewPill(ctx: CanvasRenderingContext2D, w: number, accent: string, p: CanvasPalette): HitRegion {
  const label = "View";
  ctx.font = `600 11px ${FONT}`;
  const bw = ctx.measureText(label).width + 18;
  const bh = 19;
  const bx = w - PAD - bw;
  const by = PAD - 5;
  ctx.fillStyle = p.chipBg;
  roundRectPath(ctx, bx, by, bw, bh, bh / 2);
  ctx.fill();
  ctx.strokeStyle = accent;
  ctx.lineWidth = 1;
  roundRectPath(ctx, bx, by, bw, bh, bh / 2);
  ctx.stroke();
  ctx.fillStyle = accent;
  ctx.textBaseline = "middle";
  ctx.fillText(label, bx + 9, by + bh / 2 + 0.5);
  ctx.textBaseline = "alphabetic";
  return { x: bx - 2, y: by - 2, w: bw + 4, h: bh + 4, action: { type: "expand" } };
}

export function drawCard(
  ctx: CanvasRenderingContext2D,
  content: CardContent,
  w: number,
  h: number,
  level: CardLevel,
  p: CanvasPalette,
  expandable = true,
): HitRegion[] {
  const regions: HitRegion[] = [{ x: 0, y: 0, w, h, action: { type: "node" } }];
  const accent = content.accent ?? p.accent;

  ctx.clearRect(0, 0, w, h);
  roundRectPath(ctx, 1, 1, w - 2, h - 2, 12);
  ctx.fillStyle = p.cardBgElev;
  ctx.fill();
  if (content.kind === "task") {
    ctx.save();
    roundRectPath(ctx, 1, 1, w - 2, h - 2, 12);
    ctx.clip();
    ctx.fillStyle = accent;
    ctx.fillRect(0, 0, 4, h);
    ctx.restore();
  }
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = content.kind === "task" ? accent : p.border;
  roundRectPath(ctx, 1, 1, w - 2, h - 2, 12);
  ctx.stroke();

  ctx.save();
  roundRectPath(ctx, 2, 2, w - 4, h - 4, 11);
  ctx.clip();

  // screen), render a readable inline DOCUMENT — bigger type + the body — not NEAR blown up.
  const deepW = worldThresholds(content.kind)[2];
  if (w < 54 || h < 30) drawFarLabel(ctx, content, w, h, accent, p);
  else if (w < 128 || h < 66) drawCompact(ctx, content, w, h, accent, p);
  else if (level === "far") drawFar(ctx, content, w, h, accent, p);
  else if (w >= deepW && h >= 220) drawDeep(ctx, content, w, h, accent, p, regions, expandable);
  else drawNear(ctx, content, w, h, accent, p, regions, expandable);

  ctx.restore();
  return regions;
}

function drawFarLabel(ctx: CanvasRenderingContext2D, content: CardContent, w: number, h: number, accent: string, p: CanvasPalette): void {
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  const pad = Math.max(3, Math.min(6, w * 0.12));
  const availW = w - pad * 2;
  let kp = Math.max(7, Math.min(11, Math.round(h * 0.42)));
  ctx.font = `700 ${kp}px ${FONT}`;
  while (kp > 6.5 && ctx.measureText(content.kicker).width > availW) { kp -= 0.5; ctx.font = `700 ${kp}px ${FONT}`; }
  if (h >= 12 && ctx.measureText(content.kicker).width <= availW) {
    const top = pad + kp;
    ctx.fillStyle = accent;
    ctx.fillText(content.kicker, pad, top);
    const titlePx = Math.max(7, Math.min(9, Math.round(kp - 1)));
    const ty = top + 3 + titlePx;
    if (ty + 2 <= h) {
      ctx.font = `600 ${titlePx}px ${FONT}`;
      ctx.fillStyle = p.fg;
      ctx.fillText(ellipsize(measureWith(ctx), content.title, availW), pad, ty);
    }
    return;
  }
  // Too small for the word — a single clean kind initial, centred.
  const initial = content.kicker.charAt(0);
  const ip = Math.max(8, Math.min(13, Math.round(Math.min(w, h) * 0.62)));
  ctx.font = `700 ${ip}px ${FONT}`;
  ctx.fillStyle = accent;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  if (w >= 9 && h >= 9 && ctx.measureText(initial).width <= w - 2) ctx.fillText(initial, w / 2, h / 2 + 0.5);
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
}

// no leading dot — the same header the near card uses, just tighter. Ellipsised, contained.
function drawCompact(ctx: CanvasRenderingContext2D, content: CardContent, w: number, h: number, accent: string, p: CanvasPalette): void {
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  const pad = 8;
  const kickerPx = Math.max(8, Math.min(10, Math.round(h * 0.17)));
  ctx.font = `700 ${kickerPx}px ${FONT}`;
  ctx.fillStyle = accent;
  ctx.fillText(ellipsize(measureWith(ctx), content.kicker, w - pad * 2), pad, pad + kickerPx);

  const titlePx = Math.max(9, Math.min(11, Math.round(h * 0.2)));
  const lineH = titlePx + 3;
  const top = pad + kickerPx + 5;
  const maxLines = Math.max(1, Math.min(2, Math.floor((h - top - 4) / lineH)));
  ctx.font = `600 ${titlePx}px ${FONT}`;
  ctx.fillStyle = p.fg;
  const lines = wrapLines(measureWith(ctx), content.title, w - pad * 2, maxLines);
  let y = top + titlePx;
  for (const line of lines) {
    ctx.fillText(line, pad, y);
    y += lineH;
  }
}

function drawNear(ctx: CanvasRenderingContext2D, content: CardContent, w: number, h: number, accent: string, p: CanvasPalette, regions: HitRegion[], expandable: boolean): void {
  ctx.textBaseline = "alphabetic";
  // node can opt out (capabilities.expandable=false) and reclaim the full title width.
  let titleRight = w - PAD;
  if (expandable) {
    const openPill = drawViewPill(ctx, w, accent, p);
    regions.push(openPill);
    titleRight = openPill.x - 8;
  }

  ctx.font = `700 10px ${FONT}`;
  ctx.fillStyle = accent;
  ctx.fillText(content.kicker, PAD, PAD + 8);

  // above the content (sections/fields) — never past the box.
  const availW = Math.max(40, titleRight - PAD);
  const rows = content.sections.length || content.fields.length;
  const reserveBelow = (content.tags.length ? FIT.tagRowH : 0) + Math.min(rows, 2) * (content.sections.length ? FIT.rowH : FIT.fieldH);
  const titleTop = PAD + 22;
  const titleBudget = h - PAD - reserveBelow - titleTop;
  const maxTitleLines = Math.max(1, Math.min(2, Math.floor(titleBudget / FIT.titleLineH) || 1));
  ctx.font = `600 ${TITLE_PX}px ${FONT}`;
  ctx.fillStyle = p.fg;
  const titleLines = wrapLines(measureWith(ctx), content.title, availW, maxTitleLines);
  let y = titleTop;
  for (const line of titleLines) {
    ctx.fillText(line, PAD, y);
    y += FIT.titleLineH;
  }
  y += FIT.titleGap;

  if (content.tags.length) {
    drawChips(ctx, content.tags, PAD, y, w - PAD, 11, p);
    y += FIT.tagRowH;
  }

  const bottomLimit = h - PAD - 2;
  const totalRows = content.sections.length + content.fields.length;
  let drawn = 0;

  for (const s of content.sections) {
    if (y + FIT.rowH > bottomLimit && drawn < totalRows - 1) { drawMore(ctx, totalRows - drawn, PAD, y, p); return; }
    const rowY = y;
    ctx.fillStyle = GLYPH_ACCENT[s.glyph] ?? p.fgMuted;
    ctx.beginPath();
    ctx.arc(PAD + 4, rowY + 8, 3.2, 0, Math.PI * 2);
    ctx.fill();
    let rightX = w - PAD;
    if (s.button) {
      ctx.font = `600 11px ${FONT}`;
      const bw = ctx.measureText(s.button).width + 14;
      const bx = w - PAD - bw;
      ctx.fillStyle = p.chipBg;
      roundRectPath(ctx, bx, rowY - 1, bw, 17, 8.5);
      ctx.fill();
      ctx.fillStyle = accent;
      ctx.textBaseline = "middle";
      ctx.fillText(s.button, bx + 7, rowY + 8);
      ctx.textBaseline = "alphabetic";
      rightX = bx - 6;
      regions.push({ x: bx, y: rowY - 1, w: bw, h: 17, action: { type: "section-button", sectionId: s.sectionId } });
    }
    if (s.tags.length) {
      let dx = rightX;
      for (const tg of s.tags) {
        dx -= 9;
        if (dx < PAD + 80) break;
        ctx.fillStyle = tg.color ?? p.fgMuted;
        ctx.beginPath();
        ctx.arc(dx, rowY + 8, 3, 0, Math.PI * 2);
        ctx.fill();
      }
      rightX = dx - 6;
    }
    ctx.fillStyle = p.fg;
    ctx.font = `400 13px ${FONT}`;
    ctx.fillText(ellipsize(measureWith(ctx), s.title, Math.max(20, rightX - (PAD + 13))), PAD + 13, rowY + 12);
    regions.push({ x: 0, y: rowY, w, h: FIT.rowH, action: { type: "section", sectionId: s.sectionId } });
    y += FIT.rowH;
    drawn++;
  }

  for (const f of content.fields) {
    if (y + FIT.fieldH > bottomLimit && drawn < totalRows - 1) { drawMore(ctx, totalRows - drawn, PAD, y, p); return; }
    ctx.font = `400 11px ${FONT}`;
    ctx.fillStyle = p.fgMuted;
    ctx.fillText(f.label, PAD, y + 12);
    const lx = PAD + 58;
    ctx.font = `500 12px ${FONT}`;
    ctx.fillStyle = f.color ?? p.fg;
    ctx.fillText(ellipsize(measureWith(ctx), f.value, w - PAD - lx), lx, y + 12);
    y += FIT.fieldH;
    drawn++;
  }
}

function drawMore(ctx: CanvasRenderingContext2D, remaining: number, pad: number, y: number, p: CanvasPalette): void {
  ctx.font = `500 11px ${FONT}`;
  ctx.fillStyle = p.fgMuted;
  ctx.fillText(`+${remaining} more - View`, pad, y + 12);
}

// lives behind View). Drops fences/links/emphasis/heading marks; collapses to flowing prose.
export function cleanMarkdown(text: string): string {
  return text
    .replace(/`+/g, "")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*|~~/g, "")
    .replace(/^\s{0,3}>\s?/gm, "")
    .replace(/^#{1,6}\s*/gm, "")
    .replace(/^\s*[-*+]\s+/gm, "• ");
}

// remainder (wrapLines), and a short bg fade softens the cut. Returns the y after.
function drawBodyText(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, maxW: number, bottom: number, fontPx: number, lineH: number, p: CanvasPalette): number {
  ctx.font = `400 ${fontPx}px ${FONT}`;
  ctx.fillStyle = p.fg;
  const maxLines = Math.max(1, Math.floor((bottom - y) / lineH));
  const lines = wrapLines((t) => ctx.measureText(t).width, cleanMarkdown(text), maxW, maxLines);
  let yy = y;
  for (const line of lines) { ctx.fillText(line, x, yy + fontPx); yy += lineH; }
  if (lines.length >= maxLines) {
    const g = ctx.createLinearGradient(0, bottom - lineH, 0, bottom);
    g.addColorStop(0, "rgba(0,0,0,0)");
    g.addColorStop(1, p.cardBgElev);
    ctx.fillStyle = g;
    ctx.fillRect(x - 4, bottom - lineH, maxW + 8, lineH);
  }
  return yy;
}

function drawDeep(ctx: CanvasRenderingContext2D, content: CardContent, w: number, h: number, accent: string, p: CanvasPalette, regions: HitRegion[], expandable: boolean): void {
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  const pad = 18;
  let titleRight = w - pad;
  if (expandable) { const op = drawViewPill(ctx, w, accent, p); regions.push(op); titleRight = op.x - 10; }

  ctx.font = `700 12px ${FONT}`;
  ctx.fillStyle = accent;
  ctx.fillText(content.kicker, pad, pad + 10);

  const titlePx = Math.max(20, Math.min(Math.round(w * 0.035), 30));
  const titleLH = Math.round(titlePx * 1.25);
  ctx.font = `700 ${titlePx}px ${FONT}`;
  ctx.fillStyle = p.fg;
  let y = pad + 18 + titlePx;
  for (const line of wrapLines((t) => ctx.measureText(t).width, content.title, titleRight - pad, 3)) {
    ctx.fillText(line, pad, y);
    y += titleLH;
  }
  y += 6;

  if (content.tags.length) { drawChips(ctx, content.tags, pad, y, w - pad, 12, p); y += 26; }

  const bottom = h - pad;

  if (content.fields.length) {
    for (const f of content.fields) {
      ctx.font = `400 13px ${FONT}`;
      ctx.fillStyle = p.fgMuted;
      ctx.fillText(f.label, pad, y + 14);
      ctx.font = `600 15px ${FONT}`;
      ctx.fillStyle = f.color ?? p.fg;
      ctx.fillText(ellipsize((t) => ctx.measureText(t).width, f.value, w - pad - (pad + 92)), pad + 92, y + 14);
      y += 25;
    }
    y += 10;
    ctx.strokeStyle = p.border;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(pad, y);
    ctx.lineTo(w - pad, y);
    ctx.stroke();
    y += 18;
    ctx.font = `700 11px ${FONT}`;
    ctx.fillStyle = p.fgMuted;
    ctx.fillText("DESCRIPTION", pad, y);
    y += 18;
    if (content.body) drawBodyText(ctx, content.body, pad, y, w - pad * 2, bottom, 15, 22, p);
    else { ctx.font = `italic 14px ${FONT}`; ctx.fillStyle = p.fgMuted; ctx.fillText("No description", pad, y + 14); }
    return;
  }

  if (content.sections.length) {
    y += 2;
    for (const s of content.sections) {
      if (y > bottom - 22) break;
      ctx.font = `700 16px ${FONT}`;
      ctx.fillStyle = p.fg;
      ctx.fillText(ellipsize((t) => ctx.measureText(t).width, s.title, w - pad * 2), pad, y + 14);
      y += 24;
      if (s.body) y = drawBodyText(ctx, s.body, pad, y, w - pad * 2, bottom, 14, 21, p) + 12;
    }
    return;
  }

  if (content.body) drawBodyText(ctx, content.body, pad, y, w - pad * 2, bottom, 15, 22, p);
  else { ctx.font = `italic 14px ${FONT}`; ctx.fillStyle = p.fgMuted; ctx.fillText("No description", pad, y + 14); }
}

function drawFar(ctx: CanvasRenderingContext2D, content: CardContent, w: number, h: number, accent: string, p: CanvasPalette): void {
  const pad = Math.max(8, Math.min(w, h) * 0.08);
  ctx.textBaseline = "alphabetic";
  const kickerPx = Math.max(9, Math.min(13, w * 0.06));
  ctx.font = `700 ${kickerPx}px ${FONT}`;
  ctx.fillStyle = accent;
  ctx.fillText(ellipsize(measureWith(ctx), content.kicker, w - pad * 2), pad, pad + kickerPx);

  const titlePx = Math.max(13, Math.min(w * 0.12, h * 0.24, 30));
  const lineH = titlePx * 1.16;
  ctx.font = `700 ${titlePx}px ${FONT}`;
  ctx.fillStyle = p.fg;
  const top = pad + kickerPx + 6;
  const maxLines = Math.max(1, Math.min(3, Math.floor((h - pad - top) / lineH)));
  const lines = wrapLines(measureWith(ctx), content.title, w - pad * 2, maxLines);
  let y = top + titlePx;
  for (const line of lines) {
    ctx.fillText(line, pad, y);
    y += lineH;
  }
}

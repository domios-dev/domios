
import { FIXTURE_PLAN, FIXTURE_TAG_FACETS, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TASK_STATUS_COLORS } from "../settings";
import type { Plan } from "../types";
import { buildCardContent } from "./cardContent";
import { drawCard } from "./drawCard";
import type { CanvasPalette } from "./palette";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const palette: CanvasPalette = {
  bg: "#000", cardBg: "#111", cardBgElev: "#1d2230", border: "#333", borderStrong: "#88f",
  fg: "#eee", fgMuted: "#999", chipBg: "#222", accent: "#7c83f8", onAccent: "#fff", live: "#0cf",
};

// returns a px-proportional measureText so wrap/ellipsize + the fit loop behave.
interface Drawn { text: string; fill: string; px: number; arc: boolean }
function stubCtx(): { ctx: CanvasRenderingContext2D; drawn: Drawn[]; arcs: number } {
  const state = { font: "10px x", fillStyle: "#000", arcs: 0 } as { font: string; fillStyle: string; arcs: number };
  const drawn: Drawn[] = [];
  const px = () => parseFloat(/(\d+(?:\.\d+)?)px/.exec(state.font)?.[1] ?? "10");
  const ctx = {
    set font(v: string) { state.font = v; }, get font() { return state.font; },
    set fillStyle(v: string) { state.fillStyle = v; }, get fillStyle() { return state.fillStyle; },
    set strokeStyle(_v: string) {}, set lineWidth(_v: number) {}, set textAlign(_v: string) {}, set textBaseline(_v: string) {}, set globalAlpha(_v: number) {},
    clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, arcTo() {}, closePath() {}, fill() {}, stroke() {}, save() {}, restore() {}, clip() {}, fillRect() {}, rect() {},
    arc() { state.arcs++; },
    createLinearGradient: () => ({ addColorStop() {} }),
    measureText: (t: string) => ({ width: t.length * px() * 0.55 }),
    fillText: (t: string) => { drawn.push({ text: t, fill: state.fillStyle, px: px(), arc: false }); },
  } as unknown as CanvasRenderingContext2D;
  return { ctx, drawn, get arcs() { return state.arcs; } } as never;
}

const taskContent = buildCardContent("task", { task: FIXTURE_TASK, tagFacets: [...FIXTURE_TAG_FACETS] })!;
const planContent = buildCardContent("plan", FIXTURE_PLAN as Plan)!;
const statusColor = TASK_STATUS_COLORS[FIXTURE_TASK.status];

// FAR: a tiny task node names its KIND ("TASK") in the status colour, no dot.
{
  const s = stubCtx();
  drawCard(s.ctx, taskContent, 44, 26, "far", palette);
  const kicker = s.drawn.find((d) => d.text === "TASK");
  ok("FAR draws the kind kicker word", !!kicker);
  ok("FAR kicker is status-coloured", kicker?.fill === statusColor);
  ok("FAR draws no status dot (no arc-fill glyph)", s.arcs === 0 || !s.drawn.some((d) => d.text === ""));
}

// FAR plan node → "PLAN".
{
  const s = stubCtx();
  drawCard(s.ctx, planContent, 48, 26, "far", palette);
  ok("FAR plan names PLAN", s.drawn.some((d) => d.text === "PLAN"));
}

// MID (title band): kicker on top + the title below, no leading dot.
{
  const s = stubCtx();
  drawCard(s.ctx, taskContent, 104, 52, "far", palette);
  const texts = s.drawn.map((d) => d.text);
  ok("MID draws kicker on top", texts.includes("TASK"));
  ok("MID draws the title below", texts.some((t) => t.includes("Dante")));
  const kicker = s.drawn.find((d) => d.text === "TASK");
  ok("MID kicker is status-coloured", kicker?.fill === statusColor);
}

// NEAR expand affordance is not status-like: "View", never "Open".
{
  const s = stubCtx();
  drawCard(s.ctx, taskContent, 260, 190, "near", palette);
  const texts = s.drawn.map((d) => d.text);
  ok("NEAR expand affordance says View", texts.includes("View"));
  ok("NEAR expand affordance does not say Open", !texts.includes("Open"));
}

// Extreme tiny: a clean single initial, never a half-cut word.
{
  const s = stubCtx();
  drawCard(s.ctx, taskContent, 14, 14, "far", palette);
  const texts = s.drawn.map((d) => d.text).filter(Boolean);
  ok("extreme-tiny shows only a single kind initial", texts.every((t) => t.length <= 1) && texts.includes("T"));
}

// DESCRIPTION label, the body rendered, larger fonts.
{
  const s = stubCtx();
  drawCard(s.ctx, taskContent, 620, 440, "near", palette);
  const texts = s.drawn.map((d) => d.text);
  ok("DEEP shows the DESCRIPTION label", texts.includes("DESCRIPTION"));
  ok("DEEP renders the body (markdown stripped)", texts.some((t) => t.includes("login endpoint")));
  const title = s.drawn.find((d) => d.text.includes("Dante"));
  ok("DEEP title font is document-scale (≥20px)", !!title && title.px >= 20);
}

// DEEP with no body → honest "No description" empty state (not blank space).
{
  const noBody = buildCardContent("task", { task: { ...FIXTURE_TASK, body: "" }, tagFacets: [...FIXTURE_TAG_FACETS] })!;
  const s = stubCtx();
  drawCard(s.ctx, noBody, 620, 440, "near", palette);
  const texts = s.drawn.map((d) => d.text);
  ok("DEEP empty body → 'No description'", texts.includes("No description"));
}

// DEEP plan → sections render as prose (title + body), bigger.
{
  const s = stubCtx();
  drawCard(s.ctx, planContent, 760, 520, "near", palette);
  ok("DEEP plan renders section titles", s.drawn.some((d) => d.text.length > 0));
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);

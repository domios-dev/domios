import { useLayoutEffect, useRef } from "react";
import { SpillCanvas, useBreakout } from "../../breakout";
import {
  DOT_MATRIX_GRID_STEP,
  DOT_MATRIX_REST_ALPHA,
  DOT_MATRIX_REST_RADIUS,
  DOT_MATRIX_WAVE_BAND_WIDTH,
  DOT_MATRIX_WAVE_DURATION_MS,
  DOT_MATRIX_WAVE_GROWTH,
  DOT_MATRIX_WAVE_PEAK_ALPHA,
  sampleDotMatrixWave,
  type DotMatrixWave,
} from "../../../../../src/components/brand/dotMatrixModel";
import { REST, surfaceTension, type BrandPose } from "../../../../../src/components/brand/motion";
import { BEAT_PEAK, beatLevel } from "../../../../../src/components/brand/useBrandMotion";
import { useStage } from "../../engine";
import { H, W } from "./meshFixtures";

/** Dots are soft and tiny; past this many backing pixels per stage pixel nobody sees a difference. */
const FIELD_DENSITY = 1.5;

/** Film frames are 2.4x the product's stage, so grid, dots and bands scale together. */
export const BRAND_SCALE = 2.4;
export const WAVE_PERIOD = DOT_MATRIX_WAVE_DURATION_MS / 1000;

/** Seconds of the product's surface-tension opening shown before the beat takes over; the tail after this is visually at rest. */
const OPEN_SECONDS = 0.9;

export type MarkFrame = { pose: BrandPose; beat: number | null };

/** The BrandMark load motion (open, then one beat per dot wave) as a pure function of t. */
export function markFrame(t: number, openAt: number): MarkFrame {
  const s = t - openAt;
  if (s < OPEN_SECONDS) return { pose: surfaceTension(Math.max(0, s)), beat: null };
  const cycle = ((s - OPEN_SECONDS) % WAVE_PERIOD) / WAVE_PERIOD;
  return { pose: REST, beat: beatLevel(cycle) };
}

/** Release times (s) of the dot waves the mark emits at each beat peak, as useBrandMotion schedules them. */
export function waveReleases(openAt: number, until: number): number[] {
  const out: number[] = [];
  for (let at = openAt + OPEN_SECONDS + BEAT_PEAK * WAVE_PERIOD; at <= until; at += WAVE_PERIOD) out.push(at);
  return out;
}

/** The product BrandMark path, posed by the brand motion model instead of its rAF loop. */
export function PosedMark({ frame, size }: { frame: MarkFrame; size: number }) {
  const { pose, beat } = frame;
  const transform = beat !== null
    ? `translate(32 32) scale(${0.94 + 0.1 * beat}) translate(-32 -32)`
    : pose === REST ? undefined : `translate(32 ${40 + pose.y}) scale(${pose.scaleX} ${pose.scaleY}) translate(-32 -40)`;
  return (
    <svg viewBox="-4 -4 72 72" width={size} height={size} aria-hidden="true" className="brand-mark" style={{ opacity: beat !== null ? 0.5 + 0.5 * beat : 1 }}>
      <g transform={transform}><path d={pose.path} fill="currentColor" fillRule="evenodd" /></g>
    </svg>
  );
}

type FieldProps = { t: number; releases: number[]; intensity?: number; restIntensity?: number; frameWidth?: number; frameHeight?: number; originX?: number; originY?: number };

/**
 * DotMatrixCanvas's draw pass with waves sampled at t instead of performance.now().
 * Waves originate where the mark sits: the frame centre unless an origin is given.
 */
interface Dots { t: number; releases: number[]; intensity: number; restIntensity: number; centerX: number; centerY: number; extent: number }

/** Opacity steps a dot snaps to, so a frame is a few filled paths rather than one fill per dot. */
const ALPHA_LEVELS = 24;

/** Rest dots never change on their own; a frame with no wave in flight is the same picture as the last one. */
function hasWaves(dots: Dots): boolean {
  const now = dots.t * 1000;
  return dots.releases.some((at) => sampleDotMatrixWave({ releasedAt: at * 1000, originX: dots.centerX, originY: dots.centerY, maxRadius: dots.extent }, now) !== null);
}

/** Paints the grid dots whose centres fall in [x0, x1) x [y0, y1), in brand units; the grid stays aligned outside the frame. */
function paintDots(context: CanvasRenderingContext2D, dots: Dots, x0: number, y0: number, x1: number, y1: number) {
  const { t, releases, intensity, restIntensity, centerX, centerY, extent } = dots;
  const now = t * 1000;
  const waves = releases
    .map((at): DotMatrixWave => ({ releasedAt: at * 1000, originX: centerX, originY: centerY, maxRadius: extent }))
    .flatMap((wave) => {
      const sample = sampleDotMatrixWave(wave, now);
      return sample ? [{ wave, ...sample }] : [];
    });
  const first = (from: number) => DOT_MATRIX_GRID_STEP / 2 + Math.ceil((from - DOT_MATRIX_GRID_STEP / 2) / DOT_MATRIX_GRID_STEP) * DOT_MATRIX_GRID_STEP;
  const paths: (Path2D | undefined)[] = [];
  for (let y = first(y0); y < y1; y += DOT_MATRIX_GRID_STEP) {
    for (let x = first(x0); x < x1; x += DOT_MATRIX_GRID_STEP) {
      let alpha = DOT_MATRIX_REST_ALPHA * restIntensity * Math.max(0, 1 - Math.hypot(x - centerX, y - centerY) / extent);
      let growth = 0;
      for (const { wave, radius, fade } of waves) {
        const band = (Math.hypot(x - wave.originX, y - wave.originY) - radius) / DOT_MATRIX_WAVE_BAND_WIDTH;
        const strength = Math.exp(-0.5 * band * band) * fade;
        alpha = Math.max(alpha, DOT_MATRIX_WAVE_PEAK_ALPHA * intensity * strength);
        growth = Math.max(growth, DOT_MATRIX_WAVE_GROWTH * intensity * strength);
      }
      const level = Math.ceil(Math.min(1, alpha) * ALPHA_LEVELS);
      if (level <= 0) continue;
      const path = (paths[level] ??= new Path2D());
      const r = DOT_MATRIX_REST_RADIUS * (1 + growth);
      path.moveTo(x + r, y);
      path.arc(x, y, r, 0, Math.PI * 2);
    }
  }
  paths.forEach((path, level) => {
    if (!path) return;
    context.globalAlpha = level / ALPHA_LEVELS;
    context.fill(path);
  });
  context.globalAlpha = 1;
}

export function DotField({ t, releases, intensity = 1, restIntensity = intensity, frameWidth = W, frameHeight = H, originX = frameWidth / 2, originY = frameHeight / 2 }: FieldProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const color = useRef("currentColor");
  const drawn = useRef("");
  // Read once per theme: a computed-style read every frame forces a style recalc.
  const themeRef = useRef({ key: "", color: "currentColor" });
  const margin = useBreakout()?.margin;
  const density = Math.min(useStage().density, FIELD_DENSITY);
  const width = frameWidth / BRAND_SCALE;
  const height = frameHeight / BRAND_SCALE;
  const mx = (margin?.x ?? 0) / BRAND_SCALE;
  const my = (margin?.y ?? 0) / BRAND_SCALE;
  const centerX = originX / BRAND_SCALE;
  const centerY = originY / BRAND_SCALE;
  // With room past the frame the wave keeps travelling into it instead of dying at the edge.
  const extent = Math.hypot(Math.max(centerX, width - centerX) + mx, Math.max(centerY, height - centerY) + my);
  const dots: Dots = { t, releases, intensity, restIntensity, centerX, centerY, extent };

  useLayoutEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    const bw = Math.round(frameWidth * density);
    const bh = Math.round(frameHeight * density);
    const resized = canvas.width !== bw || canvas.height !== bh;
    if (resized) {
      canvas.width = bw;
      canvas.height = bh;
    }
    const root = document.documentElement;
    const theme = `${root.getAttribute("data-ct-theme")}|${root.className}|${root.getAttribute("style")}`;
    if (themeRef.current.key !== theme) themeRef.current = { key: theme, color: getComputedStyle(canvas).color };
    const nextColor = themeRef.current.color;
    const still = hasWaves(dots) ? "" : `${bw}x${bh}|${nextColor}|${restIntensity}|${extent}|${centerX},${centerY}`;
    if (still && still === drawn.current && !resized) return;
    drawn.current = still;
    color.current = nextColor;
    context.setTransform(density * BRAND_SCALE, 0, 0, density * BRAND_SCALE, 0, 0);
    context.clearRect(0, 0, width, height);
    context.fillStyle = color.current;
    paintDots(context, dots, 0, 0, width, height);
  }, [t, releases, intensity, restIntensity, frameWidth, frameHeight, originX, originY, extent, density]);

  return (
    <>
      <canvas ref={ref} className="brand-state-matrix" aria-hidden="true" style={{ width: frameWidth, height: frameHeight }} />
      <SpillCanvas t={t} draw={(context) => {
        context.scale(BRAND_SCALE, BRAND_SCALE);
        context.fillStyle = color.current;
        paintDots(context, dots, -mx, -my, width + mx, height + my);
      }} />
    </>
  );
}

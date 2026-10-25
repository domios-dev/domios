// Grid, dot and band sizes must match DOT_WAVES in brand_fallback.rs and the boot cover in index.html.
export const DOT_MATRIX_GRID_STEP = 18;
export const DOT_MATRIX_REST_RADIUS = 1.2;
export const DOT_MATRIX_REST_ALPHA = 0.018;
export const DOT_MATRIX_WAVE_PEAK_ALPHA = 0.16;
export const DOT_MATRIX_WAVE_GROWTH = 0.3;
export const DOT_MATRIX_WAVE_DURATION_MS = 2_600;
export const DOT_MATRIX_WAVE_BAND_WIDTH = 26;

export type DotMatrixWave = {
  releasedAt: number;
  originX: number;
  originY: number;
  maxRadius: number;
};

export type DotMatrixMode = "loading" | "error";

export function emitDotMatrixWave(
  waves: readonly DotMatrixWave[],
  wave: DotMatrixWave,
  mode: DotMatrixMode,
  reducedMotion: boolean,
): readonly DotMatrixWave[] {
  if (mode !== "loading" || reducedMotion) return waves;
  return [...waves, wave];
}

export function sampleDotMatrixWave(wave: DotMatrixWave, now: number, durationMs = DOT_MATRIX_WAVE_DURATION_MS) {
  const progress = (now - wave.releasedAt) / durationMs;
  if (progress < 0 || progress >= 1) return null;
  return {
    progress,
    radius: wave.maxRadius * progress,
    fade: 1 - progress,
  };
}

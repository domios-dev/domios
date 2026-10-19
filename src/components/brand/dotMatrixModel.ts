export const DOT_MATRIX_GRID_STEP = 11;
export const DOT_MATRIX_REST_RADIUS = 0.8;
export const DOT_MATRIX_REST_ALPHA = 0.025;
export const DOT_MATRIX_WAVE_PEAK_ALPHA = 0.28;
export const DOT_MATRIX_WAVE_GROWTH = 0.3;
export const DOT_MATRIX_WAVE_DURATION_MS = 2_600;
export const DOT_MATRIX_WAVE_BAND_WIDTH = 20;

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

export function sampleDotMatrixWave(wave: DotMatrixWave, now: number) {
  const progress = (now - wave.releasedAt) / DOT_MATRIX_WAVE_DURATION_MS;
  if (progress < 0 || progress >= 1) return null;
  return {
    progress,
    radius: wave.maxRadius * progress,
    fade: 1 - progress,
  };
}

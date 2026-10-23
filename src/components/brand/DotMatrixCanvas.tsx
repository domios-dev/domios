import { forwardRef, useImperativeHandle, useLayoutEffect, useRef } from "react";
import {
  DOT_MATRIX_GRID_STEP,
  DOT_MATRIX_REST_ALPHA,
  DOT_MATRIX_REST_RADIUS,
  DOT_MATRIX_WAVE_BAND_WIDTH,
  DOT_MATRIX_WAVE_GROWTH,
  DOT_MATRIX_WAVE_PEAK_ALPHA,
  emitDotMatrixWave,
  sampleDotMatrixWave,
} from "./dotMatrixModel";
import type { DotMatrixMode, DotMatrixWave } from "./dotMatrixModel";

export type DotMatrixCanvasHandle = { emitRelease: (releasedAt: number) => void };

type DotMatrixCanvasProps = {
  mode: DotMatrixMode;
  reducedMotion: boolean;
  intensity?: number;
  /** The resting field's strength; small surfaces lift it because the rest glow is tuned for full stages. */
  restIntensity?: number;
};

export const DotMatrixCanvas = forwardRef<DotMatrixCanvasHandle, DotMatrixCanvasProps>(function DotMatrixCanvas(
  { mode, reducedMotion, intensity = 1, restIntensity = intensity },
  ref,
) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const emitRef = useRef<(releasedAt: number) => void>(() => {});
  const wavesRef = useRef<readonly DotMatrixWave[]>([]);

  useImperativeHandle(ref, () => ({ emitRelease: releasedAt => emitRef.current(releasedAt) }), []);

  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    const container = canvas?.parentElement;
    const context = canvas?.getContext("2d");
    if (!canvas || !container || !context) return;

    wavesRef.current = [];
    let frame: number | null = null;
    let width = 0;
    let height = 0;
    let pixelRatio = 1;
    let disposed = false;

    const visible = () => !disposed && !document.hidden;
    const measure = () => {
      const bounds = container.getBoundingClientRect();
      width = bounds.width;
      height = bounds.height;
      pixelRatio = window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
      canvas.width = Math.round(width * pixelRatio);
      canvas.height = Math.round(height * pixelRatio);
      context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    };
    const origin = () => {
      const canvasBounds = canvas.getBoundingClientRect();
      const markBounds = container.querySelector("[data-dot-origin], .brand-mark")?.getBoundingClientRect();
      const x = markBounds ? markBounds.left + markBounds.width / 2 - canvasBounds.left : width / 2;
      const y = markBounds ? markBounds.top + markBounds.height / 2 - canvasBounds.top : height / 2;
      const maxRadius = Math.max(
        Math.hypot(x, y),
        Math.hypot(width - x, y),
        Math.hypot(x, height - y),
        Math.hypot(width - x, height - y),
      );
      return { x, y, maxRadius };
    };
    const draw = (now: number) => {
      context.clearRect(0, 0, width, height);
      context.fillStyle = getComputedStyle(canvas).color;
      const centerX = width / 2;
      const centerY = height / 2;
      const restExtent = Math.max(
        Math.hypot(centerX, centerY),
        Math.hypot(width - centerX, centerY),
        Math.hypot(centerX, height - centerY),
        Math.hypot(width - centerX, height - centerY),
      );
      const activeWaves = wavesRef.current.flatMap(wave => {
        const sample = sampleDotMatrixWave(wave, now);
        return sample ? [{ wave, ...sample }] : [];
      });
      wavesRef.current = activeWaves.map(({ wave }) => wave);

      for (let y = DOT_MATRIX_GRID_STEP / 2; y < height; y += DOT_MATRIX_GRID_STEP) {
        for (let x = DOT_MATRIX_GRID_STEP / 2; x < width; x += DOT_MATRIX_GRID_STEP) {
          const restDistance = Math.hypot(x - centerX, y - centerY);
          let alpha = DOT_MATRIX_REST_ALPHA * restIntensity * Math.max(0, 1 - restDistance / restExtent);
          let growth = 0;

          for (const { wave, radius, fade } of activeWaves) {
            const distance = Math.hypot(x - wave.originX, y - wave.originY);
            const bandDistance = (distance - radius) / DOT_MATRIX_WAVE_BAND_WIDTH;
            const waveStrength = Math.exp(-0.5 * bandDistance * bandDistance) * fade;
            alpha = Math.max(alpha, DOT_MATRIX_WAVE_PEAK_ALPHA * intensity * waveStrength);
            growth = Math.max(growth, DOT_MATRIX_WAVE_GROWTH * intensity * waveStrength);
          }

          if (alpha === 0) continue;
          context.globalAlpha = alpha;
          context.beginPath();
          context.arc(x, y, DOT_MATRIX_REST_RADIUS * (1 + growth), 0, Math.PI * 2);
          context.fill();
        }
      }
      context.globalAlpha = 1;
      return activeWaves.length > 0;
    };
    const schedule = () => {
      if (frame === null && visible() && wavesRef.current.length > 0) {
        frame = requestAnimationFrame(tick);
      }
    };
    function tick(now: number) {
      frame = null;
      if (!visible()) return;
      if (draw(now)) schedule();
    }
    const resize = () => {
      measure();
      draw(performance.now());
      schedule();
    };
    const visibilityChange = () => {
      if (!visible()) {
        if (frame !== null) cancelAnimationFrame(frame);
        frame = null;
      } else {
        draw(performance.now());
        schedule();
      }
    };

    emitRef.current = releasedAt => {
      if (mode !== "loading" || reducedMotion || !visible()) return;
      const { x, y, maxRadius } = origin();
      wavesRef.current = emitDotMatrixWave(wavesRef.current, {
        releasedAt,
        originX: x,
        originY: y,
        maxRadius,
      }, mode, reducedMotion);
      draw(releasedAt);
      schedule();
    };

    measure();
    draw(performance.now());
    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(container);
    window.addEventListener("resize", resize);
    document.addEventListener("visibilitychange", visibilityChange);

    return () => {
      disposed = true;
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
      wavesRef.current = [];
      emitRef.current = () => {};
      resizeObserver.disconnect();
      window.removeEventListener("resize", resize);
      document.removeEventListener("visibilitychange", visibilityChange);
    };
  }, [mode, reducedMotion, intensity, restIntensity]);

  return <canvas ref={canvasRef} className="brand-state-matrix" aria-hidden="true" data-wave-enabled={mode === "loading" && !reducedMotion} />;
});

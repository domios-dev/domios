import { useLayoutEffect, useRef } from "react";
import { REST, surfaceTension } from "./motion";
import type { BrandPose } from "./motion";
import { DOT_MATRIX_WAVE_DURATION_MS } from "./dotMatrixModel";

export type BrandMotion = "static" | "load";

/** Fraction of the period where the mark is brightest; each dot wave leaves the mark here. */
export const BEAT_PEAK = 0.08;

/** One beat per dot wave: sharp rise, short fall, rest. Must match the pulse keyframes and DOT_WAVES in codeterm-service-core startup/brand_fallback.{html,rs}. */
export function beatLevel(cycleProgress: number): number {
  const rise = BEAT_PEAK, fall = 0.35;
  if (cycleProgress < rise) return 1 - (1 - cycleProgress / rise) ** 3;
  if (cycleProgress < fall) {
    const t = (cycleProgress - rise) / (fall - rise);
    return 1 - t * t * (3 - 2 * t);
  }
  return 0;
}

/** beatEvery stretches the rest between beats: 2 beats (and releases a wave) every second wave period. */
export function useBrandMotion(motion: BrandMotion, onRelease?: (timestamp: number) => void, beatEvery = 1) {
  const ref = useRef<SVGSVGElement>(null);
  const onReleaseRef = useRef(onRelease);
  onReleaseRef.current = onRelease;

  useLayoutEffect(() => {
    const svg = ref.current;
    const path = svg?.querySelector("path");
    const group = svg?.querySelector("g");
    if (!svg || !path || !group) return;
    if (typeof matchMedia !== "function") return;

    const preference = matchMedia("(prefers-reduced-motion: reduce)");
    const motionOwner = svg.closest("[data-motion]");
    let frame: number | null = null;
    let phaseStarted = 0;
    let cycleStarted = 0;
    let beats = 0;
    let phase: "rest" | "open" | "beat" = "rest";
    let disposed = false;

    const enabled = () => !disposed && !preference.matches && !document.hidden && motionOwner?.getAttribute("data-motion") !== "paused";
    const paint = (pose: BrandPose) => {
      path.setAttribute("d", pose.path);
      if (pose === REST) group.removeAttribute("transform");
      else group.setAttribute("transform", `translate(32 ${40 + pose.y}) scale(${pose.scaleX} ${pose.scaleY}) translate(-32 -40)`);
      svg.style.opacity = "";
      svg.dataset.brandState = phase;
    };
    const paintBeat = (level: number) => {
      path.setAttribute("d", REST.path);
      const scale = 0.94 + 0.1 * level;
      group.setAttribute("transform", `translate(32 32) scale(${scale}) translate(-32 -32)`);
      svg.style.opacity = String(0.5 + 0.5 * level);
      svg.dataset.brandState = phase;
    };
    const reset = () => {
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
      phase = "rest";
      paint(REST);
    };
    const schedule = () => { frame = requestAnimationFrame(tick); };
    function tick(timestamp: number) {
      frame = null;
      if (!enabled()) return reset();
      if (phase === "open") {
        const pose = surfaceTension(Math.max(0, (timestamp - phaseStarted) / 1000));
        if (pose === REST) {
          phase = "beat";
          cycleStarted = timestamp;
          beats = 0;
        } else paint(pose);
      }
      if (phase === "beat") {
        const elapsed = timestamp - cycleStarted;
        const period = DOT_MATRIX_WAVE_DURATION_MS * beatEvery;
        const peakOffset = BEAT_PEAK * DOT_MATRIX_WAVE_DURATION_MS;
        if (elapsed >= peakOffset) {
          const cycle = Math.floor((elapsed - peakOffset) / period);
          if (cycle >= beats) {
            beats = cycle + 1;
            onReleaseRef.current?.(cycleStarted + peakOffset + cycle * period);
          }
        }
        const inCycle = elapsed % period;
        paintBeat(inCycle < DOT_MATRIX_WAVE_DURATION_MS ? beatLevel(inCycle / DOT_MATRIX_WAVE_DURATION_MS) : 0);
      } else if (phase !== "open") return;
      schedule();
    }
    const startLoad = () => {
      if (motion !== "load" || phase !== "rest" || !enabled()) return;
      phase = "open";
      phaseStarted = performance.now();
      paint(surfaceTension(0));
      schedule();
    };
    const sync = () => {
      if (!enabled()) reset();
      else startLoad();
    };

    paint(REST);
    const observer = new MutationObserver(sync);
    if (motionOwner) observer.observe(motionOwner, { attributes: true, attributeFilter: ["data-motion"] });
    document.addEventListener("visibilitychange", sync);
    preference.addEventListener("change", sync);
    startLoad();

    return () => {
      disposed = true;
      reset();
      observer.disconnect();
      document.removeEventListener("visibilitychange", sync);
      preference.removeEventListener("change", sync);
    };
  }, [motion, beatEvery]);

  return ref;
}

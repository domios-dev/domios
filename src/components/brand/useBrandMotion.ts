import { useLayoutEffect, useRef } from "react";
import { compression, liquidEasing, release, REST, surfaceTension } from "./motion";
import type { BrandPose } from "./motion";

export type BrandMotion = "static" | "load";

export function useBrandMotion(motion: BrandMotion, onRelease?: (timestamp: number) => void) {
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
    let phase: "rest" | "open" | "compress" | "release" | "pause" = "rest";
    let releaseAmount = 0;
    let disposed = false;
    let ease = (progress: number) => progress;

    const enabled = () => !disposed && !preference.matches && !document.hidden && motionOwner?.getAttribute("data-motion") !== "paused";
    const paint = (pose: BrandPose) => {
      path.setAttribute("d", pose.path);
      if (pose === REST) group.removeAttribute("transform");
      else group.setAttribute("transform", `translate(32 ${40 + pose.y}) scale(${pose.scaleX} ${pose.scaleY}) translate(-32 -40)`);
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
      const seconds = Math.max(0, (timestamp - phaseStarted) / 1000);
      if (phase === "open") {
        const pose = surfaceTension(seconds);
        if (pose === REST) {
          phase = "compress";
          phaseStarted = timestamp;
          paint(REST);
        } else paint(pose);
      } else if (phase === "compress") {
        const progress = Math.min(1, seconds / .32);
        releaseAmount = .2 * ease(progress);
        paint(compression(releaseAmount));
        if (seconds >= .32) {
          phase = "release";
          phaseStarted = timestamp;
          onReleaseRef.current?.(timestamp);
        }
      } else if (phase === "release") {
        const pose = release(seconds, releaseAmount);
        if (pose === REST) {
          phase = "pause";
          phaseStarted = timestamp;
          paint(REST);
        } else paint(pose);
      } else if (phase === "pause") {
        if (seconds >= .32) {
          phase = "compress";
          phaseStarted = timestamp;
        }
      } else return;
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
    if (motion === "load") ease = liquidEasing(getComputedStyle(path).getPropertyValue("--ct-ease-liquid-settle"));
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
  }, [motion]);

  return ref;
}

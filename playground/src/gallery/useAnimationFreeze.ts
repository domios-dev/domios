import { useCallback, useLayoutEffect, useRef, useState } from "react";

const FALLBACK_CYCLE_MS = 1000;

function cycleFor(animations: Animation[]) {
  const durations = animations.map((animation) => {
    const duration = animation.effect?.getTiming().duration;
    return typeof duration === "number" && Number.isFinite(duration) ? duration : 0;
  });
  return Math.max(FALLBACK_CYCLE_MS, ...durations);
}

export function useAnimationFreeze(root: React.RefObject<HTMLElement | null>, dependencies: unknown[] = []) {
  const animations = useRef<Animation[]>([]);
  const [cycleMs, setCycleMs] = useState(FALLBACK_CYCLE_MS);
  const [count, setCount] = useState(0);

  const collect = useCallback(() => {
    const found = root.current?.getAnimations({ subtree: true }) ?? [];
    animations.current = found;
    setCount(found.length);
    setCycleMs(cycleFor(found));
    return found;
  }, [root]);

  useLayoutEffect(() => {
    collect();
  }, [collect, ...dependencies]);

  const pauseAt = useCallback((time: number) => {
    const found = collect();
    for (const animation of found) {
      animation.pause();
      animation.currentTime = time;
    }
  }, [collect]);

  const resume = useCallback(() => {
    const found = collect();
    for (const animation of found) animation.play();
  }, [collect]);

  return { animationCount: count, collect, cycleMs, pauseAt, resume };
}

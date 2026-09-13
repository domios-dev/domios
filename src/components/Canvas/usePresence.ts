import { useCallback, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { prefersReducedMotion } from "./missionControl/useReducedMotion";

export type PresenceState = "entering" | "open" | "closing";

/** Keyframe names on the presence element itself; only these settle a phase. */
export interface PresenceMotion {
  enter: string;
  exit: string;
}

export interface Presence<T, E extends HTMLElement> {
  item: T | null;
  state: PresenceState | null;
  ref: RefObject<E | null>;
}

interface Shown<T> {
  item: T;
  state: PresenceState;
}

function runsAnimation(element: HTMLElement | null, name: string): boolean {
  if (!element || prefersReducedMotion() || typeof window.getComputedStyle !== "function") return false;
  const names = String(window.getComputedStyle(element).animationName ?? "");
  return names.split(",").some((candidate) => candidate.trim() === name);
}

/** Keeps the last item mounted through its exit keyframes; a phase settles on the element's own animation event, or at once when motion is off. */
export function usePresence<T, E extends HTMLElement = HTMLElement>(
  value: T | null,
  motion: PresenceMotion,
  onExited?: (element: E | null) => void,
): Presence<T, E> {
  const [shown, setShown] = useState<Shown<T> | null>(() => (value === null ? null : { item: value, state: "entering" }));
  if (value !== null) {
    if (!shown || shown.state === "closing") setShown({ item: value, state: "entering" });
    else if (shown.item !== value) setShown({ item: value, state: shown.state });
  } else if (shown && shown.state !== "closing") {
    setShown({ item: shown.item, state: "closing" });
  }

  const ref = useRef<E | null>(null);
  const latest = useRef(shown);
  const onExitedRef = useRef(onExited);
  useLayoutEffect(() => {
    latest.current = shown;
    onExitedRef.current = onExited;
  });

  const settle = useCallback((phase: "entering" | "closing") => {
    const current = latest.current;
    if (current?.state !== phase) return;
    latest.current = phase === "closing" ? null : { item: current.item, state: "open" };
    if (phase === "closing") onExitedRef.current?.(ref.current);
    setShown((next) => {
      if (next?.state !== phase) return next;
      return phase === "closing" ? null : { item: next.item, state: "open" };
    });
  }, []);

  const mounted = shown !== null;
  useLayoutEffect(() => {
    const element = ref.current;
    if (!mounted || !element) return;
    const onAnimationSettled = (event: Event) => {
      if (event.target !== element) return;
      const name = (event as AnimationEvent).animationName;
      if (name === motion.enter) settle("entering");
      else if (name === motion.exit) settle("closing");
    };
    element.addEventListener("animationend", onAnimationSettled);
    element.addEventListener("animationcancel", onAnimationSettled);
    return () => {
      element.removeEventListener("animationend", onAnimationSettled);
      element.removeEventListener("animationcancel", onAnimationSettled);
    };
  }, [mounted, motion.enter, motion.exit, settle]);

  const phase = shown?.state ?? null;
  useLayoutEffect(() => {
    if (phase === "entering" && !runsAnimation(ref.current, motion.enter)) settle("entering");
    if (phase === "closing" && !runsAnimation(ref.current, motion.exit)) settle("closing");
  }, [phase, motion.enter, motion.exit, settle]);

  return { item: shown?.item ?? null, state: phase, ref };
}

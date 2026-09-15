import { useEffect, useRef, useState } from "react";
import { useReducedMotion } from "./useReducedMotion";

const ANIMATION_DURATION_MS = 360;

/** The one changed-value highlight shared by Mission Control and Analytics figures. */
export const CHANGED_VALUE_CLASS = "ct-number-changed";

export function useAnimatedNumber(value: number | null): number | null {
  return useTween(value, useReducedMotion());
}

function useTween(value: number | null, reducedMotion: boolean): number | null {
  const [displayValue, setDisplayValue] = useState(value);
  const displayRef = useRef(value);
  const frameRef = useRef<number | null>(null);

  useEffect(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    frameRef.current = null;

    const previous = displayRef.current;
    if (value === null || previous === null || previous === value || reducedMotion) {
      displayRef.current = value;
      setDisplayValue(value);
      return;
    }

    let startedAt: number | null = null;
    const tick = (timestamp: number) => {
      if (startedAt === null) startedAt = timestamp;
      const progress = Math.min(1, (timestamp - startedAt) / ANIMATION_DURATION_MS);
      const eased = 1 - (1 - progress) ** 3;
      const next = previous + (value - previous) * eased;
      displayRef.current = next;
      setDisplayValue(next);
      if (progress < 1) {
        frameRef.current = requestAnimationFrame(tick);
      } else {
        frameRef.current = null;
        displayRef.current = value;
        setDisplayValue(value);
      }
    };

    frameRef.current = requestAnimationFrame(tick);
    return () => {
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    };
  }, [reducedMotion, value]);

  return displayValue;
}

/** Counts real value changes; the first reading, a re-render of the same value and reduced motion never count. */
function useChangePulse(value: number | null, reducedMotion: boolean): number {
  const [seen, setSeen] = useState(value);
  const [pulse, setPulse] = useState(0);
  if (!Object.is(seen, value)) {
    setSeen(value);
    if (seen !== null && value !== null && !reducedMotion) setPulse(pulse + 1);
  }
  return pulse;
}

export interface AnimatedNumberProps {
  value: number | null;
  format?: (value: number) => string;
  className?: string;
  testId?: string;
}

export function AnimatedNumber({ value, format = formatNumber, className, testId }: AnimatedNumberProps) {
  const reducedMotion = useReducedMotion();
  const displayValue = useTween(value, reducedMotion);
  const pulse = useChangePulse(value, reducedMotion);
  const [endedPulse, setEndedPulse] = useState(0);
  const pulsing = !reducedMotion && pulse > endedPulse;
  const classes = [className, pulsing ? CHANGED_VALUE_CLASS : null].filter(Boolean).join(" ");
  // Keying by the change count restarts the CSS fade once per change and never on a same-value render.
  return (
    <span
      key={pulse}
      data-testid={testId}
      data-pulse={pulsing ? pulse : undefined}
      className={classes || undefined}
      onAnimationEnd={() => setEndedPulse(pulse)}
    >
      {displayValue === null ? "—" : format(displayValue)}
    </span>
  );
}

function formatNumber(value: number): string {
  return String(Math.round(value));
}

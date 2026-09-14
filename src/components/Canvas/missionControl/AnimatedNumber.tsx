import { useEffect, useRef, useState } from "react";
import { useReducedMotion } from "./useReducedMotion";

const ANIMATION_DURATION_MS = 360;

export function useAnimatedNumber(value: number | null): number | null {
  const reducedMotion = useReducedMotion();
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

export interface AnimatedNumberProps {
  value: number | null;
  format?: (value: number) => string;
  className?: string;
  testId?: string;
}

export function AnimatedNumber({ value, format = formatNumber, className, testId }: AnimatedNumberProps) {
  const displayValue = useAnimatedNumber(value);
  return (
    <span data-testid={testId} className={className}>
      {displayValue === null ? "—" : format(displayValue)}
    </span>
  );
}

function formatNumber(value: number): string {
  return String(Math.round(value));
}

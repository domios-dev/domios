import { useRef } from "react";

export function useStableSet<T>(next: Set<T>): Set<T> {
  const ref = useRef(next);
  const prev = ref.current;
  if (prev !== next && prev.size === next.size) {
    let same = true;
    for (const v of next) {
      if (!prev.has(v)) {
        same = false;
        break;
      }
    }
    if (same) return prev;
  }
  ref.current = next;
  return next;
}

import { useRef } from "react";

// Compare-and-reuse: if `next` has identical members to the previously returned
// Set, hand back the PRIOR reference. Kills the per-frame identity churn where a
// freshly built visible-id Set (same members) invalidates every downstream memo.
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

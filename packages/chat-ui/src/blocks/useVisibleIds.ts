import { useCallback, useEffect, useRef, useState } from "react";

const sameSet = (a: ReadonlySet<string>, b: ReadonlySet<string>) => a.size === b.size && [...a].every((id) => b.has(id));

/** Tracks which observed elements intersect the viewport; without IntersectionObserver every one counts as visible. */
export function useVisibleIds() {
  const supported = typeof IntersectionObserver !== "undefined";
  const [visible, setVisible] = useState<ReadonlySet<string>>(() => new Set());
  const idsByElement = useRef(new Map<Element, string>());
  const observer = useRef<IntersectionObserver | null>(null);
  const callbacks = useRef(new Map<string, (element: Element | null) => void>());
  useEffect(() => {
    if (!supported) return;
    const instance = new IntersectionObserver((entries) => setVisible((previous) => {
      const next = new Set(previous);
      for (const entry of entries) {
        const id = idsByElement.current.get(entry.target);
        if (id === undefined) continue;
        if (entry.isIntersecting) next.add(id); else next.delete(id);
      }
      return sameSet(next, previous) ? previous : next;
    }));
    observer.current = instance;
    for (const element of idsByElement.current.keys()) instance.observe(element);
    return () => { instance.disconnect(); observer.current = null; };
  }, [supported]);
  const observe = useCallback((id: string) => {
    let callback = callbacks.current.get(id);
    if (!callback) {
      let current: Element | null = null;
      callback = (element: Element | null) => {
        if (current === element) return;
        if (current) { observer.current?.unobserve(current); idsByElement.current.delete(current); }
        current = element;
        if (element) { idsByElement.current.set(element, id); observer.current?.observe(element); }
      };
      callbacks.current.set(id, callback);
    }
    return callback;
  }, []);
  const isVisible = useCallback((id: string) => !supported || visible.has(id), [supported, visible]);
  return { observe, isVisible };
}

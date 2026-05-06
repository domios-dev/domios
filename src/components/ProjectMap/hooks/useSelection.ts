import { useCallback, useState } from "react";

export interface SelectionState {
  selected: Set<number>;
  toggle: (id: number, additive: boolean) => void;
  add: (ids: Iterable<number>) => void;
  clear: () => void;
  set: (ids: Iterable<number>) => void;
}

export function useSelection(): SelectionState {
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const toggle = useCallback((id: number, additive: boolean) => {
    setSelected((prev) => {
      if (!additive) {
        if (prev.size === 1 && prev.has(id)) return prev;
        const fresh = new Set<number>();
        fresh.add(id);
        return fresh;
      }
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const add = useCallback((ids: Iterable<number>) => {
    setSelected((prev) => {
      const next = new Set(prev);
      for (const id of ids) next.add(id);
      return next;
    });
  }, []);

  const clear = useCallback(() => setSelected(new Set()), []);

  const set = useCallback((ids: Iterable<number>) => {
    setSelected(new Set(ids));
  }, []);

  return { selected, toggle, add, clear, set };
}

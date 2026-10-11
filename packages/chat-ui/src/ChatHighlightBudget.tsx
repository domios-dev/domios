import { createContext, useContext, useId, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode, type RefObject } from "react";
import { highlightSpanCount, reserveHighlightSpans, type HighlightNode, type HighlightReservations } from "./highlightBudget";

class HighlightBudgetStore {
  reservations: HighlightReservations = new Map();
  private listeners = new Set<() => void>();
  snapshot = () => this.reservations;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  reserve(key: string, rowId: string, spans: number) {
    const next = reserveHighlightSpans(this.reservations, key, rowId, spans);
    if (next === this.reservations) return;
    this.reservations = next;
    for (const listener of this.listeners) listener();
  }
  release(key: string) { this.reserve(key, "", 0); }
}

const HighlightContext = createContext<{ budget: HighlightBudgetStore; rowId: string } | null>(null);

export function ChatHighlightBudgetProvider({ children }: { children: ReactNode }) {
  const [budget] = useState(() => new HighlightBudgetStore());
  const value = useMemo(() => ({ budget, rowId: "rails" }), [budget]);
  return <HighlightContext.Provider value={value}>{children}</HighlightContext.Provider>;
}

export function ChatHighlightRow({ id, children }: { id: string; children: ReactNode }) {
  const scope = useContext(HighlightContext);
  const budget = scope?.budget;
  const value = useMemo(() => budget ? { budget, rowId: id } : null, [budget, id]);
  return <HighlightContext.Provider value={value}>{children}</HighlightContext.Provider>;
}

export function useHighlightVisibility(ref: RefObject<HTMLElement | null>, explicit: boolean): boolean {
  const scope = useContext(HighlightContext);
  const [visible, setVisible] = useState(false);
  useLayoutEffect(() => {
    if (!scope || typeof IntersectionObserver === "undefined") return;
    const element = ref.current?.firstElementChild;
    if (!element) return;
    const root = element.closest("[data-testid='tab-session-chat-messages']");
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting), { root });
    observer.observe(element);
    return () => observer.disconnect();
  }, [scope?.budget, ref]);
  return !scope || explicit || typeof IntersectionObserver === "undefined" || visible;
}

export function BudgetedHighlightRows({ rows, enabled, plain, render }: {
  rows: readonly HighlightNode[];
  enabled: boolean;
  plain: () => ReactNode;
  render: () => ReactNode;
}) {
  const scope = useContext(HighlightContext);
  const key = useId();
  const ownStore = useRef<HighlightBudgetStore | null>(null);
  if (!ownStore.current) ownStore.current = new HighlightBudgetStore();
  const budget = scope?.budget ?? ownStore.current;
  const reservations = useSyncExternalStore(budget.subscribe, budget.snapshot, budget.snapshot);
  const spans = enabled ? highlightSpanCount(rows) : 0;
  useLayoutEffect(() => { budget.reserve(key, scope?.rowId ?? "standalone", spans); }, [budget, key, scope?.rowId, spans, reservations]);
  useLayoutEffect(() => () => budget.release(key), [budget, key]);
  const reservation = reservations.get(key);
  return enabled && (spans === 0 || reservation?.spans === spans) ? render() : plain();
}

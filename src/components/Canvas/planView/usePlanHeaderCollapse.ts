import { useCallback, useEffect, useState } from "react";

export type HeaderWidthClass = "narrow" | "wide";

const STORAGE_PREFIX = "codeterm.planHeader.collapsed";
const NARROW_QUERY = "(max-width: 768px)";

function storageKey(widthClass: HeaderWidthClass, planId?: string): string {
  const base = `${STORAGE_PREFIX}.${widthClass}`;
  return planId ? `${base}.${planId}` : base;
}

export function readHeaderCollapsed(
  widthClass: HeaderWidthClass,
  fallback: boolean,
  planId?: string,
): boolean {
  try {
    const raw = window.localStorage.getItem(storageKey(widthClass, planId));
    if (raw === "1") return true;
    if (raw === "0") return false;
    return fallback;
  } catch {
    return fallback;
  }
}

export function writeHeaderCollapsed(
  widthClass: HeaderWidthClass,
  collapsed: boolean,
  planId?: string,
): void {
  try {
    window.localStorage.setItem(storageKey(widthClass, planId), collapsed ? "1" : "0");
  } catch {
    // UI-pref persistence is best-effort; private-mode/quota failures must not block the toggle.
  }
}

export function usePlanHeaderCollapse(
  widthClass: HeaderWidthClass,
  defaultCollapsed: boolean,
  planId?: string,
) {
  const [collapsed, setCollapsed] = useState(() =>
    readHeaderCollapsed(widthClass, defaultCollapsed, planId),
  );

  useEffect(() => {
    setCollapsed(readHeaderCollapsed(widthClass, defaultCollapsed, planId));
  }, [widthClass, defaultCollapsed, planId]);

  const toggle = useCallback(() => {
    setCollapsed((prev) => {
      const next = !prev;
      writeHeaderCollapsed(widthClass, next, planId);
      return next;
    });
  }, [widthClass, planId]);

  return { collapsed, toggle };
}

function detectNarrowPlanHeader(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia(NARROW_QUERY).matches;
}

export function useNarrowPlanHeader(): boolean {
  const [narrow, setNarrow] = useState(detectNarrowPlanHeader);

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(NARROW_QUERY);
    const update = () => setNarrow(query.matches);
    update();
    query.addEventListener("change", update);
    window.addEventListener("resize", update);
    return () => {
      query.removeEventListener("change", update);
      window.removeEventListener("resize", update);
    };
  }, []);

  return narrow;
}

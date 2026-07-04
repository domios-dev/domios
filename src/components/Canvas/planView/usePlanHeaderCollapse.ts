import { useCallback, useEffect, useState } from "react";

export type HeaderWidthClass = "narrow" | "wide";

const STORAGE_PREFIX = "codeterm.planHeader.collapsed";
const NARROW_QUERY = "(max-width: 768px)";

function storageKey(widthClass: HeaderWidthClass): string {
  return `${STORAGE_PREFIX}.${widthClass}`;
}

export function readHeaderCollapsed(widthClass: HeaderWidthClass, fallback: boolean): boolean {
  try {
    const raw = window.localStorage.getItem(storageKey(widthClass));
    if (raw === "1") return true;
    if (raw === "0") return false;
    return fallback;
  } catch {
    return fallback;
  }
}

export function writeHeaderCollapsed(widthClass: HeaderWidthClass, collapsed: boolean): void {
  try {
    window.localStorage.setItem(storageKey(widthClass), collapsed ? "1" : "0");
  } catch {
    // UI-pref persistence is best-effort; private-mode/quota failures must not block the toggle.
  }
}

export function usePlanHeaderCollapse(widthClass: HeaderWidthClass, defaultCollapsed: boolean) {
  const [collapsed, setCollapsed] = useState(() => readHeaderCollapsed(widthClass, defaultCollapsed));

  useEffect(() => {
    setCollapsed(readHeaderCollapsed(widthClass, defaultCollapsed));
  }, [widthClass, defaultCollapsed]);

  const toggle = useCallback(() => {
    setCollapsed((prev) => {
      const next = !prev;
      writeHeaderCollapsed(widthClass, next);
      return next;
    });
  }, [widthClass]);

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

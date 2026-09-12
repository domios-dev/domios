import { useCallback, useEffect, useState } from "react";

// The single breakpoint for the sidebar's two shapes; `.ct-sidebar-drawer` in globals.css matches it.
export const PHONE_VIEWPORT_QUERY = "(max-width: 640px)";

const SLIDE_OUT_MS = 300;

export function isPhoneViewport(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia(PHONE_VIEWPORT_QUERY).matches;
}

export function usePhoneViewport(): boolean {
  const [phone, setPhone] = useState(isPhoneViewport);
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(PHONE_VIEWPORT_QUERY);
    const update = () => setPhone(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  return phone;
}

export interface SidebarViewport {
  /** True while the sidebar is an overlay drawer rather than an in-flow column. */
  phone: boolean;
  open: boolean;
  /** Kept mounted through the drawer's slide-out so the transition can finish. */
  mounted: boolean;
  shown: boolean;
  toggle: () => void;
  close: () => void;
  /** Closes the drawer after a selection, but only where it overlays the content. */
  closeOnPhone: () => void;
}

export function useSidebarViewport(): SidebarViewport {
  const phone = usePhoneViewport();
  const [open, setOpen] = useState(() => !isPhoneViewport());
  // Crossing the breakpoint owns the sidebar: phone width yields the viewport to the content, desktop restores the column.
  useEffect(() => {
    setOpen(!phone);
  }, [phone]);

  const [mounted, setMounted] = useState(open);
  const [shown, setShown] = useState(open);
  useEffect(() => {
    if (open) {
      setMounted(true);
      const frame = requestAnimationFrame(() => setShown(true));
      return () => cancelAnimationFrame(frame);
    }
    setShown(false);
    // The desktop sidebar is in-flow and does not slide, so it leaves immediately.
    if (!phone) {
      setMounted(false);
      return;
    }
    const timer = window.setTimeout(() => setMounted(false), SLIDE_OUT_MS);
    return () => window.clearTimeout(timer);
  }, [open, phone]);

  const close = useCallback(() => setOpen(false), []);
  const toggle = useCallback(() => setOpen((value) => !value), []);
  const closeOnPhone = useCallback(() => {
    if (isPhoneViewport()) setOpen(false);
  }, []);

  return { phone, open, mounted, shown, toggle, close, closeOnPhone };
}

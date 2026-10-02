import { useEffect, useRef, useState } from "react";

export function useActivityPanel() {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 0, right: 8, maxHeight: 400 });
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const close = () => {
    setOpen(false);
    button.current?.focus();
  };
  useEffect(() => {
    if (!open) return;
    const place = () => {
      const rect = button.current?.getBoundingClientRect();
      if (rect) setPosition({
        top: rect.bottom,
        right: Math.max(8, window.innerWidth - rect.right),
        maxHeight: Math.max(0, window.innerHeight - rect.bottom - 8),
      });
    };
    place();
    panel.current?.focus();
    const key = (event: KeyboardEvent) => {
      if (event.code === "Escape") {
        setOpen(false);
        button.current?.focus();
      }
    };
    const click = (event: MouseEvent) => {
      if (!panel.current?.contains(event.target as Node) && !button.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("keydown", key);
    document.addEventListener("mousedown", click);
    window.addEventListener("resize", place);
    return () => {
      document.removeEventListener("keydown", key);
      document.removeEventListener("mousedown", click);
      window.removeEventListener("resize", place);
    };
  }, [open]);
  return { open, button, panel, position, close, toggle: () => setOpen((previous) => !previous) };
}

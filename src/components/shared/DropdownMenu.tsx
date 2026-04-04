import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

export interface DropdownMenuItem {
  label: string;
  icon?: ReactNode;
  onClick: () => void;
  danger?: boolean;
}

interface DropdownMenuProps {
  items: DropdownMenuItem[];
  trigger: ReactNode;
  /** Extra class on the trigger wrapper */
  className?: string;
}

/**
 * Compact dropdown menu anchored to a trigger element.
 * Renders via portal to escape overflow/transform containers.
 * Closes on click outside, Escape, or item click.
 */
export function DropdownMenu({ items, trigger, className }: DropdownMenuProps) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const calcPos = useCallback(() => {
    if (!triggerRef.current) return null;
    const rect = triggerRef.current.getBoundingClientRect();
    const menuHeight = items.length * 28 + 8;
    const spaceBelow = window.innerHeight - rect.bottom;
    const top = spaceBelow < menuHeight ? rect.top - menuHeight : rect.bottom + 2;
    // Anchor to trigger left, clamp to viewport
    const menuWidth = menuRef.current?.offsetWidth || 160;
    const left = Math.max(4, Math.min(rect.left, window.innerWidth - menuWidth - 8));
    return { top, left };
  }, [items.length]);

  const toggle = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    setOpen((v) => {
      if (!v) setPos(calcPos());
      return !v;
    });
  }, [calcPos]);

  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

  // Reposition after render (menu width now known)
  useEffect(() => {
    if (!open) return;
    // RAF to let the menu render and measure
    const id = requestAnimationFrame(() => setPos(calcPos()));
    return () => cancelAnimationFrame(id);
  }, [open, calcPos]);

  // Close on click outside
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node) &&
          triggerRef.current && !triggerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handler, true);
    return () => document.removeEventListener("mousedown", handler, true);
  }, [open]);

  // Close on Escape
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [open]);

  return (
    <>
      <div ref={triggerRef} onMouseDown={toggle} className={className}>
        {trigger}
      </div>
      {open && pos && createPortal(
        <>
          {/* Invisible backdrop to catch clicks */}
          {/* Backdrop — pointer-events:none so it doesn't block hover on menu items.
              Closing is handled by the mousedown document listener above. */}
          <div
            ref={menuRef}
            className="fixed z-50 w-max min-w-[140px] border border-[rgba(255,255,255,0.12)] rounded-md shadow-lg py-1 ct-float backdrop-blur-xl"
            style={{ top: pos.top, left: pos.left, background: "var(--ct-surface, #1e1e2e)" }}
          >
            {items.map((item, i) => (
              <button
                key={i}
                onClick={(e) => {
                  e.stopPropagation();
                  setOpen(false);
                  item.onClick();
                }}
                className={`flex items-center gap-2 w-full px-3 py-1.5 text-[11px] whitespace-nowrap transition-colors rounded-sm mx-0.5 ${
                  item.danger
                    ? "text-[var(--ct-muted-fg)] hover:bg-[rgba(239,68,68,0.15)] hover:text-[var(--ct-red)]"
                    : "text-[var(--ct-fg)] hover:bg-[rgba(255,255,255,0.08)]"
                }`}
              >
                {item.icon && <span className="shrink-0 w-3 h-3 flex items-center justify-center">{item.icon}</span>}
                {item.label}
              </button>
            ))}
          </div>
        </>,
        document.body
      )}
    </>
  );
}

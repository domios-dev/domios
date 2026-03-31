import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

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
 * Closes on click outside, Escape, or item click.
 */
export function DropdownMenu({ items, trigger, className }: DropdownMenuProps) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const toggle = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    // Force re-read position on every open
    setOpen((v) => {
      if (!v && triggerRef.current) {
        const rect = triggerRef.current.getBoundingClientRect();
        const menuHeight = items.length * 28 + 8;
        const spaceBelow = window.innerHeight - rect.bottom;
        const top = spaceBelow < menuHeight ? rect.top - menuHeight : rect.bottom + 2;
        const left = Math.max(4, Math.min(rect.right - 140, window.innerWidth - 148));
        setPos({ top, left });
      }
      return !v;
    });
  }, [items.length]);

  // Position state — anchored below trigger, flipped if near bottom
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

  useEffect(() => {
    if (!open || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    const menuHeight = items.length * 28 + 8; // estimate
    const spaceBelow = window.innerHeight - rect.bottom;
    const top = spaceBelow < menuHeight ? rect.top - menuHeight : rect.bottom + 2;
    // Align right edge with trigger right edge, but clamp to viewport
    const left = Math.max(4, Math.min(rect.right - 140, window.innerWidth - 148));
    setPos({ top, left });
  }, [open, items.length]);

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
      {open && pos && (
        <>
          {/* Invisible backdrop to catch clicks */}
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div
            ref={menuRef}
            className="fixed z-50 w-[140px] bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-md shadow-lg py-1 ct-float"
            style={{ top: pos.top, left: pos.left }}
          >
            {items.map((item, i) => (
              <button
                key={i}
                onClick={(e) => {
                  e.stopPropagation();
                  setOpen(false);
                  item.onClick();
                }}
                className={`flex items-center gap-2 w-full px-3 py-1.5 text-[11px] whitespace-nowrap transition-colors ${
                  item.danger
                    ? "text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10"
                    : "text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
                }`}
              >
                {item.icon && <span className="shrink-0 w-3 h-3 flex items-center justify-center">{item.icon}</span>}
                {item.label}
              </button>
            ))}
          </div>
        </>
      )}
    </>
  );
}

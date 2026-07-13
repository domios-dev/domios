import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

export interface DropdownMenuItem {
  label: string;
  icon?: ReactNode;
  onClick: () => void;
  danger?: boolean;
  /** Renders inert (no click, no hover state) with reduced opacity — e.g. a
   *  group child's close action, replaced by an explanation via `title`. */
  disabled?: boolean;
  /** Native tooltip text — mainly meant for the `disabled` explanation. */
  title?: string;
  "data-testid"?: string;
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
    const menuHeight = Math.min(items.length * 28 + 8, window.innerHeight - 8);
    const spaceBelow = window.innerHeight - rect.bottom;
    const preferredTop = spaceBelow < menuHeight ? rect.top - menuHeight : rect.bottom + 2;
    const top = Math.max(4, Math.min(preferredTop, window.innerHeight - menuHeight - 4));
    const menuWidth = Math.min(menuRef.current?.offsetWidth || 160, window.innerWidth - 8);
    const left = Math.max(4, Math.min(rect.left, window.innerWidth - menuWidth - 4));
    return { top, left, maxHeight: Math.max(80, window.innerHeight - top - 4) };
  }, [items.length]);

  const toggle = useCallback((e: React.MouseEvent | React.PointerEvent) => {
    e.stopPropagation();
    e.preventDefault();
    setOpen((v) => {
      if (!v) setPos(calcPos());
      return !v;
    });
  }, [calcPos]);

  const [pos, setPos] = useState<{ top: number; left: number; maxHeight: number } | null>(null);

  // Reposition after render (menu width now known)
  useEffect(() => {
    if (!open) return;
    const id = requestAnimationFrame(() => setPos(calcPos()));
    return () => cancelAnimationFrame(id);
  }, [open, calcPos]);

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
      <div
        ref={triggerRef}
        onPointerDown={toggle}
        onPointerUp={(e) => e.stopPropagation()}
        className={className}
      >
        {trigger}
      </div>
      {open && pos && createPortal(
        <>
          {/* Invisible backdrop to catch clicks */}
          {/* Backdrop — pointer-events:none so it doesn't block hover on menu items.
              Closing is handled by the mousedown document listener above. */}
          <div
            ref={menuRef}
            className="fixed z-50 w-max min-w-[140px] max-w-[calc(100vw-0.5rem)] border border-[rgba(255,255,255,0.12)] rounded-md shadow-lg py-1 ct-float backdrop-blur-xl"
            style={{
              top: pos.top,
              left: pos.left,
              maxHeight: pos.maxHeight,
              overflowY: "auto",
              background: "var(--ct-surface, #1e1e2e)",
            }}
          >
            {items.map((item, i) => (
              <button
                key={i}
                data-testid={item["data-testid"]}
                title={item.title}
                disabled={item.disabled}
                onClick={(e) => {
                  e.stopPropagation();
                  if (item.disabled) return;
                  setOpen(false);
                  item.onClick();
                }}
                className={`flex items-center gap-2 w-[calc(100%-0.25rem)] px-3 py-1.5 text-[11px] whitespace-nowrap overflow-hidden text-ellipsis transition-colors rounded-sm mx-0.5 ${
                  item.disabled
                    ? "text-[var(--ct-muted-fg)] opacity-50 cursor-not-allowed"
                    : item.danger
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

export interface DropdownSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** Native-title suffix appended to the label (e.g. " (not installed)"). */
  suffix?: string;
}

/**
 * A value-picker built on {@link DropdownMenu}. Same portal-rendered menu
 * (escapes transform/overflow-hidden ancestors and layers cleanly above the
 * WebGL xterm canvas), so it replaces native `<select>` without the native
 * popup's compositor flicker over the canvas. The trigger shows the current
 * selection; each option sets its value on click, the selected one carries a
 * check.
 */
export function DropdownSelect({
  options,
  value,
  onChange,
  disabled,
  placeholder,
  leading,
  className,
  "data-testid": testId,
}: {
  options: DropdownSelectOption[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  /** Leading adornment inside the trigger (e.g. a provider AgentIcon). */
  leading?: ReactNode;
  className?: string;
  "data-testid"?: string;
}) {
  const selected = options.find((o) => o.value === value);
  const label = selected?.label ?? placeholder ?? "";
  const items: DropdownMenuItem[] = options.map((o) => ({
    label: o.suffix ? `${o.label}${o.suffix}` : o.label,
    disabled: o.disabled,
    icon: o.value === value ? <Check size={12} /> : <span className="w-3 h-3" />,
    onClick: () => onChange(o.value),
    "data-testid": testId ? `${testId}-option-${o.value}` : undefined,
  }));

  const trigger = (
    <button
      type="button"
      data-testid={testId}
      data-value={value}
      disabled={disabled}
      aria-haspopup="listbox"
      className={`flex items-center gap-1.5 w-full min-w-0 px-2 py-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] text-[12px] text-[var(--ct-fg)] outline-none disabled:opacity-40 ${className ?? ""}`}
    >
      {leading}
      <span className="flex-1 min-w-0 truncate text-left">{label}</span>
      <ChevronDown size={12} className="shrink-0 text-[var(--ct-muted-fg)]" />
    </button>
  );

  if (disabled) return trigger;
  return <DropdownMenu items={items} trigger={trigger} className="w-full min-w-0" />;
}

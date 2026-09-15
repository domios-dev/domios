import { useCallback, useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, ChevronLeft, ChevronRight } from "lucide-react";
import { clampFloatingPosition } from "./floatingPosition";

const DROPDOWN_OPEN_EVENT = "codeterm:dropdown-open";
const MENU_ITEM_SELECTOR =
  '[role="menuitem"]:not(:disabled), [role="menuitemradio"]:not(:disabled), [role="menuitemcheckbox"]:not(:disabled)';
const MENU_NAVIGATION_KEYS = new Set(["ArrowDown", "ArrowUp", "Home", "End"]);
const TRIGGER_FOCUSABLE = 'button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])';

export interface DropdownMenuItem {
  label: string;
  icon?: ReactNode;
  onClick?: () => void;
  submenu?: {
    label: string;
    description?: string;
    items: DropdownMenuItem[];
  };
  danger?: boolean;
  /** Renders inert (no click, no hover state) with reduced opacity — e.g. a
   *  group child's close action, replaced by an explanation via `title`. */
  disabled?: boolean;
  /** Native tooltip text — mainly meant for the `disabled` explanation. */
  title?: string;
  /** Present ⇒ the item is one choice in a set: `menuitemradio` + `aria-checked`. */
  selected?: boolean;
  /** Present ⇒ an independent on/off item: `menuitemcheckbox` + `aria-checked`. */
  checked?: boolean;
  "data-testid"?: string;
}

function menuItemRole(item: DropdownMenuItem): "menuitem" | "menuitemradio" | "menuitemcheckbox" {
  if (item.checked !== undefined) return "menuitemcheckbox";
  return item.selected === undefined ? "menuitem" : "menuitemradio";
}

interface DropdownMenuProps {
  items: DropdownMenuItem[];
  trigger: ReactNode;
  /** Extra class on the trigger wrapper */
  className?: string;
  header?: ReactNode;
  contextMenuTargetRef?: RefObject<HTMLElement | null>;
  menuLabel?: string;
  menuTestId?: string;
}

/**
 * Compact dropdown menu anchored to a trigger element.
 * Renders via portal to escape overflow/transform containers.
 * Closes on click outside, Escape, or item click.
 */
export function DropdownMenu({
  items,
  trigger,
  className,
  header,
  contextMenuTargetRef,
  menuLabel = "Actions",
  menuTestId,
}: DropdownMenuProps) {
  const [open, setOpen] = useState(false);
  const focusOnOpenRef = useRef(false);
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [activeSubmenu, setActiveSubmenu] = useState<DropdownMenuItem["submenu"]>(undefined);
  const ownerRef = useRef(Symbol("dropdown"));
  const openSourceRef = useRef<"trigger" | "context">("trigger");
  const contextPointRef = useRef({ x: 0, y: 0 });
  const visibleItems = activeSubmenu?.items ?? items;

  const calcPos = useCallback(() => {
    if (!triggerRef.current) return null;
    const rect = triggerRef.current.getBoundingClientRect();
    const menuHeight = Math.min(visibleItems.length * 28 + 8 + (activeSubmenu ? 54 : header ? 42 : 0), window.innerHeight - 8);
    const menuWidth = Math.min(menuRef.current?.offsetWidth || 160, window.innerWidth - 8);
    const spaceBelow = window.innerHeight - rect.bottom;
    const preferredTop = spaceBelow < menuHeight ? rect.top - menuHeight : rect.bottom + 2;
    return clampFloatingPosition(rect.left, preferredTop, menuWidth, menuHeight, window.innerWidth, window.innerHeight);
  }, [activeSubmenu, header, visibleItems.length]);

  const contextPos = useCallback((x: number, y: number) => {
    const menuHeight = Math.min(visibleItems.length * 28 + 8 + (activeSubmenu ? 54 : header ? 42 : 0), window.innerHeight - 8);
    const menuWidth = Math.min(menuRef.current?.offsetWidth || 176, window.innerWidth - 8);
    return clampFloatingPosition(x, y, menuWidth, menuHeight, window.innerWidth, window.innerHeight);
  }, [activeSubmenu, header, visibleItems.length]);

  const announceOpen = useCallback(() => {
    document.dispatchEvent(new window.CustomEvent(DROPDOWN_OPEN_EVENT, { detail: ownerRef.current }));
  }, []);

  const toggle = useCallback((e: React.MouseEvent | React.PointerEvent | React.KeyboardEvent, focusFirstItem = false) => {
    e.stopPropagation();
    e.preventDefault();
    if (open) {
      setOpen(false);
      return;
    }
    announceOpen();
    openSourceRef.current = "trigger";
    focusOnOpenRef.current = focusFirstItem;
    setActiveSubmenu(undefined);
    setPos(calcPos());
    setOpen(true);
  }, [announceOpen, calcPos, open]);

  const [pos, setPos] = useState<{ top: number; left: number; maxHeight: number } | null>(null);

  const closeMenu = useCallback((restoreFocus: boolean) => {
    setOpen(false);
    setActiveSubmenu(undefined);
    if (restoreFocus) triggerRef.current?.querySelector<HTMLElement>(TRIGGER_FOCUSABLE)?.focus();
  }, []);

  const moveFocus = useCallback((key: string) => {
    const menuItems = Array.from(menuRef.current?.querySelectorAll<HTMLElement>(MENU_ITEM_SELECTOR) ?? []);
    if (menuItems.length === 0) return;
    const current = menuItems.indexOf(document.activeElement as HTMLElement);
    const next = key === "Home"
      ? 0
      : key === "End"
        ? menuItems.length - 1
        : key === "ArrowDown"
          ? (current + 1) % menuItems.length
          : (current <= 0 ? menuItems.length : current) - 1;
    menuItems[next]?.focus();
  }, []);

  useEffect(() => {
    if (!open || !pos || !focusOnOpenRef.current) return;
    focusOnOpenRef.current = false;
    moveFocus("Home");
  }, [moveFocus, open, pos]);

  useEffect(() => {
    if (!open) return;
    const id = requestAnimationFrame(() => {
      setPos(openSourceRef.current === "context"
        ? contextPos(contextPointRef.current.x, contextPointRef.current.y)
        : calcPos());
    });
    return () => cancelAnimationFrame(id);
  }, [open, calcPos, contextPos]);

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
    const handleOtherDropdown = (event: Event) => {
      if ((event as CustomEvent<symbol>).detail === ownerRef.current) return;
      setOpen(false);
      setActiveSubmenu(undefined);
    };
    document.addEventListener(DROPDOWN_OPEN_EVENT, handleOtherDropdown);
    return () => document.removeEventListener(DROPDOWN_OPEN_EVENT, handleOtherDropdown);
  }, []);

  useEffect(() => {
    const target = contextMenuTargetRef?.current;
    if (!target) return;
    const handleContextMenu = (event: MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
      announceOpen();
      openSourceRef.current = "context";
      focusOnOpenRef.current = false;
      contextPointRef.current = { x: event.clientX, y: event.clientY };
      setActiveSubmenu(undefined);
      setPos(contextPos(event.clientX, event.clientY));
      setOpen(true);
    };
    target.addEventListener("contextmenu", handleContextMenu);
    return () => target.removeEventListener("contextmenu", handleContextMenu);
  }, [announceOpen, contextMenuTargetRef, contextPos]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      if (activeSubmenu) {
        setActiveSubmenu(undefined);
        return;
      }
      const active = document.activeElement;
      // A pointer open leaves focus where it was; when that is nowhere, Escape lands on the trigger rather than body.
      const focusLost = !active || active === document.body;
      closeMenu(focusLost || Boolean(menuRef.current?.contains(active) || triggerRef.current?.contains(active)));
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [activeSubmenu, closeMenu, open]);

  return (
    <>
      <div
        ref={triggerRef}
        onPointerDown={toggle}
        onPointerUp={(e) => e.stopPropagation()}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(event) => {
          if ((event.key === "Enter" || event.key === " ") && !event.repeat) {
            toggle(event, true);
            return;
          }
          if (!open || (event.key !== "Escape" && event.key !== "ArrowDown")) return;
          event.preventDefault();
          event.stopPropagation();
          if (event.key === "Escape") closeMenu(true);
          else moveFocus("Home");
        }}
        className={className}
      >
        {trigger}
      </div>
      {open && pos && createPortal(
        <>
          <div
            ref={menuRef}
            role="menu"
            aria-label={menuLabel}
            data-testid={menuTestId}
            onKeyDown={(event) => {
              event.stopPropagation();
              if (event.key === "Escape") {
                event.preventDefault();
                if (activeSubmenu) setActiveSubmenu(undefined);
                else closeMenu(true);
                return;
              }
              if (event.key === "Tab") {
                closeMenu(true);
                return;
              }
              if (!MENU_NAVIGATION_KEYS.has(event.key)) return;
              event.preventDefault();
              moveFocus(event.key);
            }}
            className="ct-portal-menu fixed w-max min-w-[140px] max-w-[calc(100vw-0.5rem)] border border-[rgba(255,255,255,0.12)] rounded-md shadow-lg py-1 ct-float backdrop-blur-xl"
            style={{
              top: pos.top,
              left: pos.left,
              maxHeight: pos.maxHeight,
              overflowY: "auto",
              background: "var(--ct-surface, #1e1e2e)",
            }}
          >
            {activeSubmenu ? (
              <div className="border-b border-[var(--ct-border)] px-1 pb-1 pt-0.5">
                <button
                  type="button"
                  role="menuitem"
                  className="flex w-full items-center gap-1.5 rounded-sm px-2 py-1 text-left text-[11px] text-[var(--ct-fg)] hover:bg-[rgba(255,255,255,0.08)]"
                  onClick={() => setActiveSubmenu(undefined)}
                >
                  <ChevronLeft aria-hidden size={11} />
                  {activeSubmenu.label}
                </button>
                {activeSubmenu.description && (
                  <div className="px-2 pb-1 text-[10px] text-[var(--ct-muted-fg)]">{activeSubmenu.description}</div>
                )}
              </div>
            ) : header}
            {visibleItems.map((item, i) => (
              <button
                key={`${item.label}:${i}`}
                role={menuItemRole(item)}
                aria-checked={item.checked ?? item.selected}
                aria-haspopup={item.submenu ? "menu" : undefined}
                aria-expanded={item.submenu ? activeSubmenu === item.submenu : undefined}
                data-testid={item["data-testid"]}
                data-danger={item.danger ? "true" : undefined}
                title={item.title}
                disabled={item.disabled}
                onClick={(e) => {
                  e.stopPropagation();
                  if (item.disabled) return;
                  if (item.submenu) {
                    setActiveSubmenu(item.submenu);
                    return;
                  }
                  setOpen(false);
                  item.onClick?.();
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
                <span className="min-w-0 flex-1 truncate text-left">{item.label}</span>
                {item.submenu && <ChevronRight aria-hidden size={11} className="shrink-0 text-[var(--ct-muted-fg)]" />}
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

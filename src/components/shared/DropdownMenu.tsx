import { useCallback, useEffect, useId, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, ChevronLeft, ChevronRight } from "lucide-react";
import { clampFloatingPosition } from "./floatingPosition";
import { groupOptionsByLabel } from "../../lib/optionGroups";

const DROPDOWN_OPEN_EVENT = "codeterm:dropdown-open";
const MENU_ITEM_SELECTOR =
  '[role="menuitem"]:not(:disabled), [role="menuitemradio"]:not(:disabled), [role="menuitemcheckbox"]:not(:disabled)';
const MENU_NAVIGATION_KEYS = new Set(["ArrowDown", "ArrowUp", "Home", "End"]);
const TRIGGER_FOCUSABLE = 'button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])';
const ROW_HEIGHT = 28;
const DESCRIPTION_HEIGHT = 18;
const SEPARATOR_HEIGHT = 9;
const GROUP_HEADER_HEIGHT = 24;

export interface DropdownMenuItem {
  label: string;
  icon?: ReactNode;
  /** Short chip rendered beside the label (e.g. a model's catalog badge). */
  badge?: string;
  onClick?: () => void;
  submenu?: {
    label: string;
    description?: string;
    items: DropdownMenuEntry[];
  };
  danger?: boolean;
  /** Renders inert (no click, no hover state) with reduced opacity — e.g. a
   *  group child's close action, replaced by an explanation via `title`. */
  disabled?: boolean;
  /** Inert but still focusable, so its `description` is announced as the reason. */
  unavailable?: boolean;
  /** Native tooltip text — mainly meant for the `disabled` explanation. */
  title?: string;
  /** Secondary line, exposed through `aria-describedby`. */
  description?: string;
  shortcut?: string;
  /** Keeps the menu open after activation so a toggle shows its new state. */
  keepOpen?: boolean;
  /** Present ⇒ the item is one choice in a set: `menuitemradio` + `aria-checked`. */
  selected?: boolean;
  /** Present ⇒ an independent on/off item: `menuitemcheckbox` + `aria-checked`. */
  checked?: boolean;
  "data-testid"?: string;
}

export interface DropdownMenuSeparator {
  separator: true;
  key: string;
}

/** A collapsible section of menu items under one heading. */
export interface DropdownMenuGroup {
  /** Heading text; an empty label renders the items with no header. */
  label: string;
  items: DropdownMenuItem[];
  /** Stable identity for collapse state; defaults to `label`. */
  groupKey?: string;
  "data-testid"?: string;
}

export type DropdownMenuEntry = DropdownMenuItem | DropdownMenuSeparator | DropdownMenuGroup;

function isSeparator(entry: DropdownMenuEntry): entry is DropdownMenuSeparator {
  return "separator" in entry;
}

function isGroup(entry: DropdownMenuEntry): entry is DropdownMenuGroup {
  return "items" in entry;
}

function groupKeyOf(group: DropdownMenuGroup): string {
  return group.groupKey ?? group.label;
}

function itemKey(item: DropdownMenuItem): string {
  return item["data-testid"] ?? item.label;
}

function menuItemRole(item: DropdownMenuItem): "menuitem" | "menuitemradio" | "menuitemcheckbox" {
  if (item.checked !== undefined) return "menuitemcheckbox";
  return item.selected === undefined ? "menuitem" : "menuitemradio";
}

/** Every actionable item, flattening groups — submenu lookups and focus walks
 *  operate on leaves, never on section wrappers. */
function leafItems(entries: DropdownMenuEntry[]): DropdownMenuItem[] {
  const out: DropdownMenuItem[] = [];
  for (const entry of entries) {
    if (isSeparator(entry)) continue;
    if (isGroup(entry)) out.push(...entry.items);
    else out.push(entry);
  }
  return out;
}

function estimateMenuHeight(
  entries: DropdownMenuEntry[],
  chrome: number,
  collapsed: ReadonlySet<string>,
): number {
  let rows = 0;
  for (const entry of entries) {
    if (isSeparator(entry)) {
      rows += SEPARATOR_HEIGHT;
      continue;
    }
    if (isGroup(entry)) {
      if (entry.label) rows += GROUP_HEADER_HEIGHT;
      if (!collapsed.has(groupKeyOf(entry))) {
        rows += entry.items.reduce(
          (total, item) => total + ROW_HEIGHT + (item.description ? DESCRIPTION_HEIGHT : 0),
          0,
        );
      }
      continue;
    }
    rows += ROW_HEIGHT + (entry.description ? DESCRIPTION_HEIGHT : 0);
  }
  return rows + 8 + chrome;
}

type PendingFocus = { kind: "submenu" } | { kind: "parent"; key: string };

interface DropdownMenuProps {
  items: DropdownMenuEntry[];
  trigger: ReactNode;
  /** Extra class on the trigger wrapper */
  className?: string;
  header?: ReactNode;
  contextMenuTargetRef?: RefObject<HTMLElement | null>;
  menuLabel?: string;
  menuTestId?: string;
  /** Pixel cap on the open menu; content beyond it scrolls internally. */
  maxHeight?: number;
  onOpenChange?: (open: boolean) => void;
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
  maxHeight,
  onOpenChange,
}: DropdownMenuProps) {
  const [open, setOpen] = useState(false);
  const [collapsedGroups, setCollapsedGroups] = useState<ReadonlySet<string>>(() => new Set<string>());
  const focusOnOpenRef = useRef(false);
  const pendingFocusRef = useRef<PendingFocus | null>(null);
  const onOpenChangeRef = useRef(onOpenChange);
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [submenuKey, setSubmenuKey] = useState<string | null>(null);
  const ownerRef = useRef(Symbol("dropdown"));
  const openSourceRef = useRef<"trigger" | "context">("trigger");
  const contextPointRef = useRef({ x: 0, y: 0 });
  const descriptionIdBase = useId();
  const activeSubmenu = submenuKey === null
    ? undefined
    : leafItems(items).find((item) => item.submenu !== undefined && itemKey(item) === submenuKey)?.submenu;
  const inSubmenu = activeSubmenu !== undefined;
  const visibleEntries = activeSubmenu?.items ?? items;
  const contentHeight = estimateMenuHeight(
    visibleEntries,
    inSubmenu ? 54 : header ? 42 : 0,
    collapsedGroups,
  );

  const capHeight = useCallback(
    (estimated: number) => Math.min(estimated, maxHeight ?? window.innerHeight - 8, window.innerHeight - 8),
    [maxHeight],
  );

  const calcPos = useCallback(() => {
    if (!triggerRef.current) return null;
    const menuHeight = capHeight(contentHeight);
    const rect = triggerRef.current.getBoundingClientRect();
    const menuWidth = Math.min(menuRef.current?.offsetWidth || 160, window.innerWidth - 8);
    const spaceBelow = window.innerHeight - rect.bottom;
    const preferredTop = spaceBelow < menuHeight ? rect.top - menuHeight : rect.bottom + 2;
    return clampFloatingPosition(rect.left, preferredTop, menuWidth, menuHeight, window.innerWidth, window.innerHeight);
  }, [capHeight, contentHeight]);

  const contextPos = useCallback((x: number, y: number) => {
    const menuHeight = capHeight(contentHeight);
    const menuWidth = Math.min(menuRef.current?.offsetWidth || 176, window.innerWidth - 8);
    return clampFloatingPosition(x, y, menuWidth, menuHeight, window.innerWidth, window.innerHeight);
  }, [capHeight, contentHeight]);

  const announceOpen = useCallback(() => {
    document.dispatchEvent(new window.CustomEvent(DROPDOWN_OPEN_EVENT, { detail: ownerRef.current }));
  }, []);

  useEffect(() => {
    onOpenChangeRef.current = onOpenChange;
  });

  useEffect(() => {
    onOpenChangeRef.current?.(open);
  }, [open]);

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
    pendingFocusRef.current = null;
    setSubmenuKey(null);
    setPos(calcPos());
    setOpen(true);
  }, [announceOpen, calcPos, open]);

  const [pos, setPos] = useState<{ top: number; left: number; maxHeight: number } | null>(null);

  const closeMenu = useCallback((restoreFocus: boolean) => {
    setOpen(false);
    setSubmenuKey(null);
    pendingFocusRef.current = null;
    if (restoreFocus) triggerRef.current?.querySelector<HTMLElement>(TRIGGER_FOCUSABLE)?.focus();
  }, []);

  const toggleGroup = useCallback((key: string) => {
    setCollapsedGroups((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const enterSubmenu = useCallback((key: string) => {
    pendingFocusRef.current = { kind: "submenu" };
    setSubmenuKey(key);
  }, []);

  const leaveSubmenu = useCallback(() => {
    if (submenuKey === null) return;
    pendingFocusRef.current = { kind: "parent", key: submenuKey };
    setSubmenuKey(null);
  }, [submenuKey]);

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
    const pending = pendingFocusRef.current;
    const menu = menuRef.current;
    if (!pending || !open || !menu) return;
    pendingFocusRef.current = null;
    const focusable = Array.from(menu.querySelectorAll<HTMLElement>(MENU_ITEM_SELECTOR));
    const target = pending.kind === "submenu"
      ? focusable.find((element) => !element.hasAttribute("data-menu-back")) ?? focusable[0]
      : focusable.find((element) => element.getAttribute("data-menu-key") === pending.key);
    target?.focus();
  }, [open, submenuKey]);

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
      setSubmenuKey(null);
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
      pendingFocusRef.current = null;
      contextPointRef.current = { x: event.clientX, y: event.clientY };
      setSubmenuKey(null);
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
      if (inSubmenu) {
        leaveSubmenu();
        return;
      }
      const active = document.activeElement;
      // A pointer open leaves focus where it was; when that is nowhere, Escape lands on the trigger rather than body.
      const focusLost = !active || active === document.body;
      closeMenu(focusLost || Boolean(menuRef.current?.contains(active) || triggerRef.current?.contains(active)));
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [closeMenu, inSubmenu, leaveSubmenu, open]);

  const activate = (item: DropdownMenuItem) => {
    if (item.disabled || item.unavailable) return;
    if (item.submenu) {
      enterSubmenu(itemKey(item));
      return;
    }
    if (!item.keepOpen) setOpen(false);
    item.onClick?.();
  };

  const submenuParentAt = (element: EventTarget): DropdownMenuItem | undefined => {
    const key = (element as HTMLElement).getAttribute?.("data-menu-key");
    if (!key) return undefined;
    return leafItems(visibleEntries).find(
      (item) => item.submenu !== undefined && !item.disabled && itemKey(item) === key,
    );
  };

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
                if (inSubmenu) leaveSubmenu();
                else closeMenu(true);
                return;
              }
              if (event.key === "Tab") {
                closeMenu(true);
                return;
              }
              if (event.key === "ArrowLeft" && inSubmenu) {
                event.preventDefault();
                leaveSubmenu();
                return;
              }
              if (event.key === "ArrowRight" && !inSubmenu) {
                const parent = submenuParentAt(event.target);
                if (!parent) return;
                event.preventDefault();
                enterSubmenu(itemKey(parent));
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
              maxHeight: maxHeight ? Math.min(pos.maxHeight, maxHeight) : pos.maxHeight,
              overflowY: "auto",
              background: "var(--ct-surface, #1e1e2e)",
            }}
          >
            {activeSubmenu ? (
              <div className="border-b border-[var(--ct-border)] px-1 pb-1 pt-0.5">
                <button
                  type="button"
                  role="menuitem"
                  data-menu-back=""
                  className="flex w-full items-center gap-1.5 rounded-sm px-2 py-1 text-left text-[11px] text-[var(--ct-fg)] hover:bg-[rgba(255,255,255,0.08)]"
                  onClick={(e) => {
                    e.stopPropagation();
                    leaveSubmenu();
                  }}
                >
                  <ChevronLeft aria-hidden size={11} />
                  {activeSubmenu.label}
                </button>
                {activeSubmenu.description && (
                  <div className="px-2 pb-1 text-[10px] text-[var(--ct-muted-fg)]">{activeSubmenu.description}</div>
                )}
              </div>
            ) : header}
            {visibleEntries.map((entry, i) => isSeparator(entry) ? (
              <div key={`separator:${entry.key}`} role="separator" className="mx-2 my-1 border-t border-[var(--ct-border)]" />
            ) : isGroup(entry) ? (
              <div key={`group:${groupKeyOf(entry)}`} role="group" aria-label={entry.label || undefined}>
                {entry.label && (
                  <button
                    type="button"
                    role="menuitem"
                    aria-expanded={!collapsedGroups.has(groupKeyOf(entry))}
                    data-testid={entry["data-testid"]}
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleGroup(groupKeyOf(entry));
                    }}
                    className="flex w-[calc(100%-0.25rem)] items-center gap-1 mx-0.5 px-2 py-1 text-[10px] font-medium uppercase tracking-wide text-[var(--ct-muted-fg)] hover:bg-[rgba(255,255,255,0.08)] rounded-sm"
                  >
                    {collapsedGroups.has(groupKeyOf(entry))
                      ? <ChevronRight aria-hidden size={11} />
                      : <ChevronDown aria-hidden size={11} />}
                    <span className="truncate">{entry.label}</span>
                  </button>
                )}
                {!collapsedGroups.has(groupKeyOf(entry)) && entry.items.map((item, j) => (
                  <DropdownMenuRow
                    key={`${groupKeyOf(entry)}:${item.label}:${j}`}
                    item={item}
                    descriptionId={`${descriptionIdBase}-${i}-${j}`}
                    onActivate={() => activate(item)}
                  />
                ))}
              </div>
            ) : (
              <DropdownMenuRow
                key={`${entry.label}:${i}`}
                item={entry}
                descriptionId={`${descriptionIdBase}-${i}`}
                onActivate={() => activate(entry)}
              />
            ))}
          </div>
        </>,
        document.body
      )}
    </>
  );
}

function DropdownMenuRow({
  item,
  descriptionId,
  onActivate,
}: {
  item: DropdownMenuItem;
  descriptionId: string;
  onActivate: () => void;
}) {
  const inert = item.disabled || item.unavailable;
  return (
    <button
      type="button"
      role={menuItemRole(item)}
      aria-checked={item.checked ?? item.selected}
      aria-haspopup={item.submenu ? "menu" : undefined}
      aria-expanded={item.submenu ? false : undefined}
      aria-disabled={item.unavailable || undefined}
      aria-describedby={item.description ? descriptionId : undefined}
      data-menu-key={item.submenu ? itemKey(item) : undefined}
      data-testid={item["data-testid"]}
      data-danger={item.danger ? "true" : undefined}
      title={item.title}
      disabled={item.disabled}
      onClick={(e) => {
        e.stopPropagation();
        onActivate();
      }}
      className={`flex items-center gap-2 w-[calc(100%-0.25rem)] px-3 py-1.5 text-[11px] whitespace-nowrap overflow-hidden text-ellipsis transition-colors rounded-sm mx-0.5 ${
        inert
          ? "text-[var(--ct-muted-fg)] opacity-50 cursor-not-allowed"
          : item.danger
            ? "text-[var(--ct-muted-fg)] hover:bg-[rgba(239,68,68,0.15)] hover:text-[var(--ct-red)]"
            : "text-[var(--ct-fg)] hover:bg-[rgba(255,255,255,0.08)]"
      }`}
    >
      {item.icon && <span className="shrink-0 w-3 h-3 flex items-center justify-center">{item.icon}</span>}
      <span className="min-w-0 flex-1 text-left">
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="truncate">{item.label}</span>
          {item.badge && (
            <span
              data-badge-for={item["data-testid"]}
              className="shrink-0 rounded px-1 py-px text-[9px] font-medium uppercase tracking-wide text-[var(--ct-accent)]"
              style={{ background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)" }}
            >
              {item.badge}
            </span>
          )}
        </span>
        {item.description && (
          <span id={descriptionId} className="mt-0.5 block max-w-56 whitespace-normal text-[10px] leading-snug text-[var(--ct-muted-fg)]">
            {item.description}
          </span>
        )}
      </span>
      {item.shortcut && <kbd className="shrink-0 font-mono text-[9px] text-[var(--ct-muted)]">{item.shortcut}</kbd>}
      {item.checked === true && <Check aria-hidden size={11} className="shrink-0 text-[var(--ct-accent)]" />}
      {item.submenu && <ChevronRight aria-hidden size={11} className="shrink-0 text-[var(--ct-muted-fg)]" />}
    </button>
  );
}

export interface DropdownSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** Native-title suffix appended to the label (e.g. " (not installed)"). */
  suffix?: string;
  /** Short chip rendered beside the option label. */
  badge?: string;
  /** Section heading; options sharing it render together under one collapsible
   *  header. Options without a group stay in a headerless section. */
  group?: string;
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
  prefix,
  menuMaxHeight,
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
  /** Static label shown before the value so the control names itself, e.g.
   *  "Effort" → "Effort Medium". */
  prefix?: string;
  /** Pixel cap on the open menu; longer lists scroll internally. */
  menuMaxHeight?: number;
  className?: string;
  "data-testid"?: string;
}) {
  const selected = options.find((o) => o.value === value);
  const label = selected?.label ?? placeholder ?? "";
  const badge = selected?.badge;
  const toItem = (o: DropdownSelectOption): DropdownMenuItem => ({
    label: o.suffix ? `${o.label}${o.suffix}` : o.label,
    badge: o.badge,
    disabled: o.disabled,
    icon: o.value === value ? <Check size={12} /> : <span className="w-3 h-3" />,
    onClick: () => onChange(o.value),
    "data-testid": testId ? `${testId}-option-${o.value}` : undefined,
  });
  const groups = groupOptionsByLabel(options);
  const items: DropdownMenuEntry[] = groups
    ? groups.map((section) => ({
      label: section.label,
      items: section.options.map(toItem),
      "data-testid": testId && section.label ? `${testId}-group-${section.label}` : undefined,
    }))
    : options.map(toItem);

  const trigger = (
    <button
      type="button"
      data-testid={testId}
      data-value={value}
      disabled={disabled}
      aria-haspopup="listbox"
      aria-label={prefix ? `${prefix}: ${label}` : undefined}
      className={`flex items-center gap-1.5 w-full min-w-0 px-2 py-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] text-[12px] text-[var(--ct-fg)] outline-none disabled:opacity-40 ${className ?? ""}`}
    >
      {leading}
      {prefix && (
        <span className="shrink-0 text-[10px] font-medium uppercase tracking-wide text-[var(--ct-muted-fg)]">
          {prefix}
        </span>
      )}
      <span className="flex-1 min-w-0 truncate text-left">{label}</span>
      {badge && (
        <span
          data-testid={testId ? `${testId}-badge` : undefined}
          className="shrink-0 rounded px-1 py-px text-[9px] font-medium uppercase tracking-wide text-[var(--ct-accent)]"
          style={{ background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)" }}
        >
          {badge}
        </span>
      )}
      <ChevronDown size={12} className="shrink-0 text-[var(--ct-muted-fg)]" />
    </button>
  );

  if (disabled) return trigger;
  return (
    <DropdownMenu
      items={items}
      trigger={trigger}
      className="w-full min-w-0"
      maxHeight={menuMaxHeight}
    />
  );
}

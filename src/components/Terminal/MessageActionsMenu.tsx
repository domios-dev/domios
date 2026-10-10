import { useEffect, useLayoutEffect, useRef, useState, type ComponentType, type KeyboardEvent } from "react";
import { Check, Copy, Hash, ListChecks, MessageSquarePlus } from "lucide-react";
import { messageClipboard, messageMarkdownClipboard, sourceDiffersFromVisible, type ClipMessage } from "@codeterm/chat-core";
import { writeClipboardFlavours } from "@codeterm/chat-ui";
import { createPortal } from "react-dom";

export type DotSide = "start" | "end";
type OpenedBy = "hover" | "focus" | "click";
type Placement = "below" | "above";

/** Room the menu needs under its dot before it opens upward instead. */
const MENU_ROOM_PX = 200;

export interface MessageMenuActions {
  quote: (clip: ClipMessage) => void;
  ask: (clip: ClipMessage) => void;
  select: () => void;
}

function menuPlacement(anchor: HTMLElement): Placement {
  const surface = anchor.closest<HTMLElement>("[role='log']");
  const bottom = surface ? surface.getBoundingClientRect().bottom : window.innerHeight;
  return anchor.getBoundingClientRect().bottom + MENU_ROOM_PX > bottom ? "above" : "below";
}

function MenuItem({ label, icon: Icon, hint, done, onSelect }: {
  label: string;
  icon: ComponentType<{ size?: number; "aria-hidden"?: boolean }>;
  hint?: string;
  done?: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      tabIndex={-1}
      aria-label={label}
      className="ct-menu-row"
      data-done={done || undefined}
      onClick={(event) => { event.stopPropagation(); onSelect(); }}
    >
      <span className="ct-menu-row__icon"><Icon size={14} aria-hidden /></span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {hint && <span aria-hidden className="ct-menu-row__shortcut">{hint}</span>}
    </button>
  );
}

/** The quiet gutter dot of one message and the small menu it opens on hover, focus, click or Enter. */
export function MessageActionsDot({ clip, touch = false, whole = false, hasParts = false, onScopeChange, side, open, onOpenChange, actions, hint, onHintEnd }: {
  touch?: boolean;
  whole?: boolean;
  hasParts?: boolean;
  onScopeChange?: (whole: boolean) => void;
  hint?: boolean;
  onHintEnd?: () => void;
  clip: ClipMessage;
  side: DotSide;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  actions: MessageMenuActions;
}) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const openedBy = useRef<OpenedBy | null>(null);
  const focusItem = useRef<"first" | "last" | null>(null);
  const [placement, setPlacement] = useState<Placement>("below");
  const [copied, setCopied] = useState<"copy" | "markdown" | null>(null);

  const show = (by: OpenedBy, item: "first" | "last" | null = null) => {
    if (!open) {
      if (wrapRef.current) setPlacement(menuPlacement(wrapRef.current));
      openedBy.current = by;
      onOpenChange(true);
    } else if (by !== "hover") {
      openedBy.current = by;
    }
    focusItem.current = item;
    if (open && item) focusMenuItem(item);
  };
  const hide = (refocus = false) => {
    openedBy.current = null;
    setCopied(null);
    onOpenChange(false);
    if (refocus) (touch ? wrapRef.current?.closest<HTMLElement>("[data-message-id]") : buttonRef.current)?.focus({ preventScroll: true });
  };

  const items = () => Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>("[role='menuitem'], [role='menuitemradio']") ?? []);
  const focusMenuItem = (which: "first" | "last") => {
    const list = items();
    list[which === "first" ? 0 : list.length - 1]?.focus({ preventScroll: true });
  };

  useLayoutEffect(() => {
    if (!open || (!focusItem.current && !touch)) return;
    if (!menuRef.current?.contains(document.activeElement)) focusMenuItem(focusItem.current ?? "first");
    focusItem.current = null;
  });

  useEffect(() => {
    if (!open) return;
    const onPress = (event: Event) => {
      if (touch && event.type === "mousedown") return;
      if (!wrapRef.current?.contains(event.target as Node) && !menuRef.current?.contains(event.target as Node)) hide();
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (!touch || event.code !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      hide(true);
    };
    document.addEventListener("mousedown", onPress);
    document.addEventListener("pointerdown", onPress);
    document.addEventListener("keydown", onKey);
    return () => { document.removeEventListener("mousedown", onPress); document.removeEventListener("pointerdown", onPress); document.removeEventListener("keydown", onKey); };
  });

  const copy = (kind: "copy" | "markdown") => {
    writeClipboardFlavours(kind === "copy" ? messageClipboard(clip) : messageMarkdownClipboard(clip));
    setCopied(kind);
  };

  const onDotKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.code === "Enter" || event.code === "Space" || event.code === "NumpadEnter" || event.code === "ArrowDown") {
      event.preventDefault();
      show("click", "first");
    } else if (event.code === "ArrowUp") {
      event.preventDefault();
      show("click", "last");
    } else if (event.code === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      hide();
    }
  };

  const onMenuKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const list = items();
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    const move = (index: number) => { event.preventDefault(); list[(index + list.length) % list.length]?.focus({ preventScroll: true }); };
    if (event.code === "ArrowDown") move(at + 1);
    else if (event.code === "ArrowUp") move(at < 0 ? list.length - 1 : at - 1);
    else if (event.code === "Home") move(0);
    else if (event.code === "End") move(list.length - 1);
    else if (event.code === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      hide(true);
    } else if (event.code === "Tab") hide();
  };

  const menu = open && (
        <div className={touch ? "ct-msg-sheet-wrap" : "ct-msg-menu-wrap"} data-side={side} data-placement={placement}>
          <div ref={menuRef} role="menu" aria-label="Message actions" className="ct-menu ct-msg-menu" onKeyDown={onMenuKey}>
            {touch && <div className="ct-msg-menu__preview" role="note" aria-label="Action preview">{messageClipboard(clip).plain}</div>}
            {hasParts && <div className="ct-msg-scope" role="group" aria-label="Action target">
              <button type="button" role="menuitemradio" tabIndex={-1} aria-checked={!whole} onClick={() => { setCopied(null); onScopeChange?.(false); }}>This part</button>
              <button type="button" role="menuitemradio" tabIndex={-1} aria-checked={whole} onClick={() => { setCopied(null); onScopeChange?.(true); }}>Whole message</button>
            </div>}
            <MenuItem label={copied === "copy" ? "Copied" : "Copy"} icon={copied === "copy" ? Check : Copy} done={copied === "copy"} onSelect={() => copy("copy")} />
            {sourceDiffersFromVisible(clip) && <MenuItem label={copied === "markdown" ? "Copied as Markdown" : "Copy as Markdown"} icon={copied === "markdown" ? Check : Hash} done={copied === "markdown"} onSelect={() => copy("markdown")} />}
            <div role="separator" className="ct-menu-separator" />
            <MenuItem label="Ask about this" icon={MessageSquarePlus} onSelect={() => { hide(); actions.quote(clip); }} />
            <div role="separator" className="ct-menu-separator" />
            <MenuItem label="Select" icon={ListChecks} hint={touch ? undefined : "Shift-click"} onSelect={() => { hide(); actions.select(); }} />
            {touch && <button type="button" role="menuitem" className="ct-menu-row" onClick={() => hide(true)}>Close actions</button>}
          </div>
        </div>
  );

  return (
    <div
      ref={wrapRef}
      className="ct-msg-dot"
      data-side={side}
      data-open={open || undefined}
      data-hint={hint || undefined}
      onPointerEnter={(event) => { if (event.pointerType === "mouse") show("hover"); }}
      onPointerLeave={(event) => { if (event.pointerType === "mouse" && openedBy.current === "hover") hide(); }}
      onBlur={(event) => {
        if (touch) return;
        if (openedBy.current !== "hover" && !wrapRef.current?.contains(event.relatedTarget as Node | null) && !menuRef.current?.contains(event.relatedTarget as Node | null)) hide();
      }}
    >
      <button
        ref={buttonRef}
        type="button"
        className="ct-msg-dot__button"
        aria-label="Message actions"
        aria-haspopup="menu"
        aria-expanded={open}
        onFocus={() => show("focus")}
        onClick={() => (open && openedBy.current === "click" ? hide() : show("click"))}
        onKeyDown={onDotKey}
      >
        <span aria-hidden="true" className="ct-msg-dot__more" onAnimationEnd={onHintEnd}><i /><i /><i /></span>
      </button>
      {touch ? createPortal(menu, document.body) : menu}
    </div>
  );
}

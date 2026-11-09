import { Fragment, useEffect, useLayoutEffect, useRef, useState, type ComponentType, type KeyboardEvent, type ReactNode } from "react";
import { Trash2, Check, Copy, Hash, ListChecks, MessageSquarePlus, GitBranch, RotateCcw, ArrowBigUp, MousePointer2 } from "lucide-react";
import { messageClipboard, messageMarkdownClipboard, sourceDiffersFromVisible, type ClipMessage } from "@codeterm/chat-core";
import { mergeMessageMenu, messageMenuGroup, writeClipboardFlavours, type ChatCardMenuItem } from "@codeterm/chat-ui";
import { createPortal } from "react-dom";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { useMessageHoverBridge, useMessageHoverIntent } from "./useMessageHoverBridge";

export type DotSide = "start" | "end";
type OpenedBy = "hover" | "focus" | "click";

export interface MessageMenuActions {
  quote: (clip: ClipMessage) => void;
  ask: (clip: ClipMessage) => void;
  select: () => void;
  delete?: () => void;
  fork?: () => void;
  revert?: () => void;
}

const MENU_ICON = 14;
const icon = (Icon: ComponentType<{ size?: number; "aria-hidden"?: boolean }>) => <Icon size={MENU_ICON} aria-hidden />;

const SELECT_HINT = <span className="inline-flex items-center gap-1"><kbd className="inline-flex items-center rounded border px-1 py-0.5" style={{ borderColor: "var(--ct-border)" }}><ArrowBigUp size={12} /></kbd><span>+</span><MousePointer2 size={13} /></span>;

/** The message's own actions, as the shared menu model; a card's items merge into them (see `mergeMessageMenu`). */
export function messageMenuItems(clip: ClipMessage, actions: MessageMenuActions, { touch = false }: { touch?: boolean } = {}): ChatCardMenuItem[] {
  const items: ChatCardMenuItem[] = [
    { label: "Copy", kind: "copy", icon: icon(Copy), onSelect: () => writeClipboardFlavours(messageClipboard(clip)) },
  ];
  if (sourceDiffersFromVisible(clip)) items.push({ label: "Copy as Markdown", kind: "copy-markdown", icon: icon(Hash), onSelect: () => writeClipboardFlavours(messageMarkdownClipboard(clip)) });
  items.push(
    { label: "Ask about this", kind: "ask", icon: icon(MessageSquarePlus), onSelect: () => actions.quote(clip) },
    { label: "Select", kind: "select", icon: icon(ListChecks), hint: touch ? undefined : SELECT_HINT, onSelect: () => actions.select() },
  );
  if (actions.fork) items.push({ label: "Fork from here", kind: "fork", icon: icon(GitBranch), onSelect: actions.fork });
  if (actions.revert) items.push({ label: "Revert to here", kind: "revert", icon: icon(RotateCcw), onSelect: actions.revert });
  if (actions.delete) items.push({ label: "Delete", kind: "delete", icon: icon(Trash2), hint: "Delete", onSelect: actions.delete });
  return items;
}

function MenuItem({ label, icon, hint, done, onSelect }: {
  label: string;
  icon: ReactNode;
  hint?: ReactNode;
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
      <span className="ct-menu-row__icon">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {hint && <span aria-hidden className="ct-menu-row__shortcut">{hint}</span>}
    </button>
  );
}

/** The quiet gutter dot of one message and the small menu it opens on hover, focus, click or Enter. */
export function MessageActionsDot({ metadata, clip, touch = false, whole = false, hasParts = false, onScopeChange, side, open, onOpenChange, actions, cardItems, hint, onHintEnd }: {
  /** The items of the card that speaks for this row, merged into the message's own. */
  cardItems?: () => readonly ChatCardMenuItem[];
  metadata?: { time: string; date: string; shortDate: string; dateTime: string; sender?: string; deliveryState?: string };
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
  const [copied, setCopied] = useState<"copy" | "markdown" | null>(null);
  const hoverIntent = useMessageHoverIntent(() => show("hover"));

  const show = (by: OpenedBy, item: "first" | "last" | null = null) => {
    hoverIntent.cancel();
    if (!open) {
      openedBy.current = by;
      onOpenChange(true);
    } else if (by !== "hover") {
      openedBy.current = by;
    }
    focusItem.current = item;
    if (open && item) focusMenuItem(item);
  };
  const hide = (refocus = false) => {
    hoverIntent.cancel();
    openedBy.current = null;
    setCopied(null);
    onOpenChange(false);
    if (refocus) (touch ? wrapRef.current?.closest<HTMLElement>("[data-message-id]") : buttonRef.current)?.focus({ preventScroll: true });
  };
  const onHoverLeave = useMessageHoverBridge({ open: open && !touch, anchor: buttonRef, menu: menuRef, isHover: () => openedBy.current === "hover", onClose: hide });

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
      if (event.code !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      hide(true);
    };
    document.addEventListener("mousedown", onPress);
    document.addEventListener("pointerdown", onPress);
    document.addEventListener("keydown", onKey);
    return () => { document.removeEventListener("mousedown", onPress); document.removeEventListener("pointerdown", onPress); document.removeEventListener("keydown", onKey); };
  });

  const own = messageMenuItems(clip, actions, { touch });
  const entries = mergeMessageMenu(own, open ? cardItems?.() ?? [] : []);
  // Copying keeps the menu open to say so; anything else closes it first, and a card's action (Open full screen)
  // starts from the refocused dot so focus has somewhere to come back to.
  const choose = (item: ChatCardMenuItem) => {
    if (item.kind === "copy" || item.kind === "copy-markdown") {
      item.onSelect();
      setCopied(item.kind === "copy" ? "copy" : "markdown");
      return;
    }
    const fromCard = !own.includes(item);
    hide(fromCard);
    item.onSelect();
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

  const menu = (
          <div ref={menuRef} role="menu" aria-label="Message actions" className="ct-menu ct-msg-menu" onKeyDown={onMenuKey}>
            {metadata && <div role="note" aria-label={[metadata.time, metadata.date].join(" · ")} className="border-b px-3 py-2 text-xs" style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}>
              <time dateTime={metadata.dateTime} title={metadata.date} className="block truncate">{metadata.time} · {metadata.shortDate}</time>
              {(metadata.sender || metadata.deliveryState) && <div className="truncate">{[metadata.sender, metadata.deliveryState].filter(Boolean).join(" · ")}</div>}
            </div>}
            {touch && <div className="ct-msg-menu__preview" role="note" aria-label="Action preview">{messageClipboard(clip).plain}</div>}
            {hasParts && <div className="ct-msg-scope" role="group" aria-label="Action target">
              <button type="button" role="menuitemradio" tabIndex={-1} aria-checked={!whole} onClick={() => { setCopied(null); onScopeChange?.(false); }}>This part</button>
              <button type="button" role="menuitemradio" tabIndex={-1} aria-checked={whole} onClick={() => { setCopied(null); onScopeChange?.(true); }}>Whole message</button>
            </div>}
            {entries.map((item, index) => {
              const done = (item.kind === "copy" && copied === "copy") || (item.kind === "copy-markdown" && copied === "markdown");
              const label = done ? (item.kind === "copy" ? "Copied" : "Copied as Markdown") : item.label;
              const separated = index > 0 && messageMenuGroup(entries[index - 1]) !== messageMenuGroup(item);
              return <Fragment key={`${item.kind ?? "card"}:${item.label}`}>
                {separated && <div role="separator" className="ct-menu-separator" />}
                <MenuItem label={label} icon={done ? icon(Check) : item.icon} hint={item.hint} done={done} onSelect={() => choose(item)} />
              </Fragment>;
            })}
            {touch && <button type="button" role="menuitem" className="ct-menu-row" onClick={() => hide(true)}>Close actions</button>}
          </div>
  );

  return (
    <div
      ref={wrapRef}
      className="ct-msg-dot"
      data-side={side}
      data-open={open || undefined}
      data-hint={hint || undefined}
      onPointerEnter={(event) => { if (event.pointerType === "mouse" && !open) hoverIntent.start(); }}
      onPointerLeave={(event) => { hoverIntent.cancel(); onHoverLeave(event); }}
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
        {hoverIntent.pending && !open && <svg aria-hidden="true" className="ct-msg-dot__progress" viewBox="0 0 20 20"><circle cx="10" cy="10" r="8" pathLength="1" /></svg>}
      </button>
      {touch
        ? open && createPortal(<div className="ct-msg-sheet-wrap">{menu}</div>, document.body)
        : (
          <AnchoredPopover open={open} anchor={buttonRef} gap={0} placement={side === "end" ? "bottom-end" : "bottom-start"}>
            <div className="ct-msg-menu-wrap">{menu}</div>
          </AnchoredPopover>
        )}
    </div>
  );
}

import { useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Copy, Ellipsis, RefreshCw, RotateCcw, ShieldCheck } from "lucide-react";
import { BusySpinner } from "../shared/BusyState";
import { clampFloatingPosition } from "../shared/floatingPosition";
import { TAB_BAR_CONTROL_H } from "./tabBarConstants";
import { copyTabSessionId, TAB_SESSION_LABELS } from "./tabSessionActions";
import { TelegramThreadMenuGroup } from "./TelegramThreadMenuGroup";
import type { TelegramThreadMenuState } from "./telegramThreadActions";

const MENU_WIDTH = 240;
const MENU_ITEMS = '[role="menuitem"]:not(:disabled), [role="menuitemcheckbox"]:not(:disabled)';

interface TabActionsMenuProps {
  sessionId: string | null;
  canReconnect: boolean;
  canRestart: boolean;
  reconnecting: boolean;
  restarting: boolean;
  onReconnect: () => void;
  onRestart: () => void;
  onError: (message: string) => void;
  /** Persisted auto-confirm opt-in; `null` hides the item for providers that cannot recognize prompts. */
  permissionAutoConfirm?: boolean | null;
  onTogglePermissionAutoConfirm?: (enabled: boolean) => void;
  /** Telegram thread items; `null` or absent hides the group. */
  telegramThread?: TelegramThreadMenuState | null;
  onMenuOpen?: () => void;
  onOpenTelegramThread?: () => void;
  onCloseTelegramThread?: () => void;
}

export function TabActionsMenu({
  sessionId,
  canReconnect,
  canRestart,
  reconnecting,
  restarting,
  onReconnect,
  onRestart,
  onError,
  permissionAutoConfirm = null,
  onTogglePermissionAutoConfirm,
  telegramThread = null,
  onMenuOpen,
  onOpenTelegramThread,
  onCloseTelegramThread,
}: TabActionsMenuProps) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<{ top: number; left: number; maxHeight: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const busy = reconnecting || restarting;
  const showAutoConfirm = permissionAutoConfirm !== null && onTogglePermissionAutoConfirm !== undefined;

  const place = useCallback(() => {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (!rect) return;
    setPosition(clampFloatingPosition(
      rect.right - MENU_WIDTH,
      rect.bottom + 5,
      MENU_WIDTH,
      showAutoConfirm ? 230 : 154,
      window.innerWidth,
      window.innerHeight,
      8,
    ));
  }, [showAutoConfirm]);

  const close = useCallback(() => setOpen(false), []);

  useEffect(() => {
    if (open) onMenuOpen?.();
  }, [open, onMenuOpen]);

  const toggle = useCallback(() => {
    setOpen((current) => {
      if (!current) place();
      return !current;
    });
  }, [place]);

  useEffect(() => {
    if (!open) return;
    menuRef.current?.querySelector<HTMLButtonElement>(MENU_ITEMS)?.focus();
    const onPointer = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node) && !triggerRef.current?.contains(event.target as Node)) close();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
        triggerRef.current?.focus();
      }
    };
    document.addEventListener("mousedown", onPointer, true);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", place);
    return () => {
      document.removeEventListener("mousedown", onPointer, true);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", place);
    };
  }, [close, open, place]);

  const handleMenuKeyDown = (event: React.KeyboardEvent) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const items = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>(MENU_ITEMS) ?? []);
    if (items.length === 0) return;
    event.preventDefault();
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const direction = event.key === "ArrowDown" ? 1 : -1;
    items[(current + direction + items.length) % items.length]?.focus();
  };

  const choose = (action: () => void) => {
    close();
    action();
  };

  const copySessionId = async () => {
    if (!sessionId) return;
    try {
      await copyTabSessionId(sessionId);
      close();
    } catch (error) {
      onError(error instanceof Error ? error.message : "Could not copy session ID");
    }
  };

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        data-testid="tab-actions-trigger"
        aria-label="Tab actions"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Tab actions"
        onClick={toggle}
        className="inline-flex w-7 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
        style={{ height: TAB_BAR_CONTROL_H }}
      >
        <BusySpinner spinning={busy} size={13} icon={Ellipsis} />
      </button>

      {open && position && createPortal(
        <div
          ref={menuRef}
          role="menu"
          aria-label="Tab actions"
          data-testid="tab-actions-menu"
          className="ct-menu-pop ct-float ct-portal-menu fixed w-60 overflow-hidden rounded-xl border border-[var(--ct-border)] bg-[var(--ct-surface)] p-1.5 shadow-[var(--ct-shadow-lg)]"
          style={position}
          onKeyDown={handleMenuKeyDown}
        >
          <div className="px-2 pb-1.5 pt-1">
            <div className="text-[9px] font-semibold uppercase tracking-[0.14em] text-[var(--ct-muted)]">Session</div>
            <div className="mt-0.5 flex min-w-0 items-center gap-1">
              <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-[var(--ct-muted-fg)]" title={sessionId ?? "Session ID is not available yet"}>
                {sessionId ?? "ID not available yet"}
              </span>
              <button
                type="button"
                role="menuitem"
                data-testid="tab-copy-session-id"
                aria-label={TAB_SESSION_LABELS.copy}
                title={TAB_SESSION_LABELS.copy}
                disabled={!sessionId}
                onClick={() => void copySessionId()}
                className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] disabled:cursor-not-allowed disabled:opacity-35"
              >
                <Copy size={12} />
              </button>
            </div>
          </div>
          {canReconnect && (
            <MenuItem
              testId="chat-reconnect"
              icon={<RefreshCw size={14} />}
              label={reconnecting ? "Reconnecting…" : TAB_SESSION_LABELS.reconnect}
              shortcut="Ctrl+R"
              disabled={busy}
              onClick={() => choose(onReconnect)}
            />
          )}
          {canRestart && (
            <MenuItem
              testId="tab-restart-session"
              icon={<RotateCcw size={14} />}
              label={restarting ? "Restarting…" : TAB_SESSION_LABELS.restart}
              disabled={busy}
              onClick={() => choose(onRestart)}
            />
          )}
          {showAutoConfirm && (
            <>
              <div role="separator" className="mx-2 my-1 border-t border-[var(--ct-border)]" />
              <MenuCheckItem
                testId="tab-permission-auto-confirm"
                icon={<ShieldCheck size={14} />}
                label={TAB_SESSION_LABELS.autoConfirm}
                description={TAB_SESSION_LABELS.autoConfirmHint}
                checked={permissionAutoConfirm === true}
                onToggle={() => onTogglePermissionAutoConfirm?.(permissionAutoConfirm !== true)}
              />
            </>
          )}
          {telegramThread && onOpenTelegramThread && onCloseTelegramThread && (
            <TelegramThreadMenuGroup
              state={telegramThread}
              onOpen={() => choose(onOpenTelegramThread)}
              onClose={() => choose(onCloseTelegramThread)}
            />
          )}
        </div>,
        document.body,
      )}
    </>
  );
}

const ITEM_CLASS =
  "flex w-full gap-2 rounded-lg px-2 py-2 text-left text-[12px] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] focus-visible:bg-[var(--ct-hover)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)] disabled:cursor-not-allowed disabled:opacity-40";

function MenuItem({
  testId,
  icon,
  label,
  shortcut,
  disabled,
  onClick,
}: {
  testId: string;
  icon: React.ReactNode;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      data-testid={testId}
      disabled={disabled}
      onClick={onClick}
      className={`${ITEM_CLASS} items-center`}
    >
      <span className="flex w-4 shrink-0 items-center justify-center text-[var(--ct-muted-fg)]">{icon}</span>
      <span className="min-w-0 flex-1">{label}</span>
      {shortcut && <kbd className="font-mono text-[9px] text-[var(--ct-muted)]">{shortcut}</kbd>}
    </button>
  );
}

function MenuCheckItem({
  testId,
  icon,
  label,
  description,
  checked,
  onToggle,
}: {
  testId: string;
  icon: React.ReactNode;
  label: string;
  description: string;
  checked: boolean;
  onToggle: () => void;
}) {
  const descriptionId = useId();
  return (
    <button
      type="button"
      role="menuitemcheckbox"
      aria-checked={checked}
      aria-describedby={descriptionId}
      data-testid={testId}
      onClick={onToggle}
      className={`${ITEM_CLASS} items-start`}
    >
      <span className="mt-0.5 flex w-4 shrink-0 items-center justify-center text-[var(--ct-muted-fg)]">{icon}</span>
      <span className="min-w-0 flex-1">
        <span className="block">{label}</span>
        <span id={descriptionId} className="mt-0.5 block text-[10px] leading-snug text-[var(--ct-muted-fg)]">
          {description}
        </span>
      </span>
      <span
        aria-hidden
        className="mt-0.5 inline-flex h-3.5 w-6 shrink-0 items-center rounded-full p-px transition-colors"
        style={{ background: checked ? "var(--ct-green)" : "color-mix(in srgb, var(--ct-muted-fg) 35%, transparent)" }}
      >
        <span
          className="h-2.5 w-2.5 rounded-full bg-[var(--ct-bg)] transition-transform"
          style={{ transform: checked ? "translateX(10px)" : "translateX(0)" }}
        />
      </span>
    </button>
  );
}

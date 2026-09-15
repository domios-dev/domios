import { useId } from "react";
import { MessageSquare, MessageSquareX } from "lucide-react";
import { TELEGRAM_THREAD_LABELS, telegramThreadHint, type TelegramThreadMenuState } from "./telegramThreadActions";

const ITEM_CLASS =
  "flex w-full items-start gap-2 rounded-lg px-2 py-2 text-left text-[12px] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] focus-visible:bg-[var(--ct-hover)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)] aria-disabled:cursor-not-allowed aria-disabled:opacity-40";

interface TelegramThreadMenuGroupProps {
  state: TelegramThreadMenuState;
  onOpen: () => void;
  onClose: () => void;
}

/** Telegram thread items of the tab actions menu; open stays focusable while unavailable so its reason is announced. */
export function TelegramThreadMenuGroup({ state, onOpen, onClose }: TelegramThreadMenuGroupProps) {
  const hintId = useId();
  const hint = telegramThreadHint(state);
  const openBlocked = !state.bridgeActive;

  return (
    <div role="group" aria-label={TELEGRAM_THREAD_LABELS.group} data-testid="tab-telegram-thread-group">
      <div role="separator" className="mx-2 my-1 border-t border-[var(--ct-border)]" />
      <button
        type="button"
        role="menuitem"
        data-testid="tab-telegram-thread-open"
        aria-disabled={openBlocked || undefined}
        aria-describedby={hint ? hintId : undefined}
        onClick={() => {
          if (!openBlocked) onOpen();
        }}
        className={ITEM_CLASS}
      >
        <span className="mt-0.5 flex w-4 shrink-0 items-center justify-center text-[var(--ct-muted-fg)]">
          <MessageSquare size={14} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block">{TELEGRAM_THREAD_LABELS.open}</span>
          {hint && (
            <span id={hintId} className="mt-0.5 block text-[10px] leading-snug text-[var(--ct-muted-fg)]">
              {hint}
            </span>
          )}
        </span>
      </button>
      {state.bound && (
        <button
          type="button"
          role="menuitem"
          data-testid="tab-telegram-thread-close"
          onClick={onClose}
          className={ITEM_CLASS}
        >
          <span className="mt-0.5 flex w-4 shrink-0 items-center justify-center text-[var(--ct-muted-fg)]">
            <MessageSquareX size={14} />
          </span>
          <span className="min-w-0 flex-1">{TELEGRAM_THREAD_LABELS.close}</span>
        </button>
      )}
    </div>
  );
}

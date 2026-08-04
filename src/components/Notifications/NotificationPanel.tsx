import { useCallback, useState } from "react";
import type { Notification } from "../../lib/types";
import { Bell, ChevronDown, Trash2, X } from "lucide-react";
import { timeAgo } from "../../lib/formatters";
import { UI_BUTTON, UI_TEXT } from "../shared/design";

const LEVEL_COLORS: Record<Notification["level"], string> = {
  Info: "var(--ct-accent)",
  Warning: "var(--ct-yellow)",
  Error: "var(--ct-red)",
};

interface NotificationPanelProps {
  notifications: Notification[];
  onMarkRead: (id: string) => void;
  onMarkAllRead: () => void;
  onDelete: (id: string) => void;
  onClearAll: () => void;
  onJumpToTab: (tabId: string, workspaceId: string | null) => void;
  onClose: () => void;
}

export function NotificationPanel({
  notifications,
  onMarkRead,
  onMarkAllRead,
  onDelete,
  onClearAll,
  onJumpToTab,
  onClose,
}: NotificationPanelProps) {
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set());
  const sorted = [...notifications].sort((a, b) => b.timestamp - a.timestamp);

  const handleClick = useCallback(
    (notif: Notification) => {
      if (!notif.read) onMarkRead(notif.id);
    },
    [onMarkRead],
  );
  const headerBtnClass = `${UI_BUTTON.ghost} ${UI_BUTTON.compact} h-6 px-1.5 text-[10px] disabled:pointer-events-none disabled:opacity-35`;
  const rowClass = `group flex items-start gap-2 border-b border-[var(--ct-border)] px-3 py-2.5 transition-colors hover:bg-[var(--ct-hover)]`;

  return (
    <div
      data-testid="notification-panel"
      className="flex w-[min(360px,calc(100vw-16px))] max-w-[calc(100vw-16px)] flex-col overflow-hidden rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] shadow-2xl ct-float"
      style={{ maxHeight: "min(520px, calc(100dvh - var(--ct-safe-top, 0px) - var(--ct-safe-bottom, 0px) - 48px))" }}
    >
      <div className="flex min-h-9 shrink-0 items-center justify-between border-b border-[var(--ct-border)] px-2.5 py-1">
        <span className="flex items-center gap-1.5 text-[11px] font-medium text-[var(--ct-fg)]">
          <Bell size={13} /> Notifications
        </span>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={onMarkAllRead}
            disabled={sorted.length === 0}
            className={`${headerBtnClass} text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]`}
            title="Mark all read"
          >
            Read all
          </button>
          <button
            type="button"
            onClick={onClearAll}
            disabled={sorted.length === 0}
            className={`${headerBtnClass} gap-1 text-[var(--ct-muted-fg)] hover:text-[var(--ct-red)]`}
            title="Clear all notifications"
          >
            <Trash2 size={11} />
            Clear all
          </button>
          <button
            type="button"
            onClick={onClose}
            className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-[var(--ct-muted-fg)] transition-colors hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--ct-accent)]"
            aria-label="Close notifications"
            title="Close"
          >
            <X size={15} strokeWidth={2} />
          </button>
        </div>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto">
        {sorted.length === 0 ? (
          <div className="flex h-28 flex-col items-center justify-center gap-2">
            <Bell size={18} className="text-[var(--ct-muted)]" />
            <span className={UI_TEXT.metaMuted}>No notifications</span>
          </div>
        ) : (
          sorted.map((notif) => (
            <div
              key={notif.id}
              onClick={() => handleClick(notif)}
              className={`${rowClass} ${!notif.read ? "border-l-2 border-l-[var(--ct-accent)]" : "border-l-2 border-l-transparent"}`}
            >
              <LevelDot level={notif.level} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-2">
                  <span
                    className={`${notif.read ? UI_TEXT.meta : "font-medium text-[var(--ct-fg)]"} truncate`}
                  >
                    {notif.title}
                  </span>
                  <span className="shrink-0 text-[10px] text-[var(--ct-muted)]">
                    {timeAgo(notif.timestamp)}
                  </span>
                </div>
                {notif.body && (
                  <>
                    <button
                      type="button"
                      aria-label={`${expandedIds.has(notif.id) ? "Hide" : "Show"} complete message for ${notif.title}`}
                      aria-expanded={expandedIds.has(notif.id)}
                      className="mt-1 inline-flex items-center gap-1 text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
                      onClick={(event) => {
                        event.stopPropagation();
                        setExpandedIds((current) => {
                          const next = new Set(current);
                          if (next.has(notif.id)) next.delete(notif.id);
                          else next.add(notif.id);
                          return next;
                        });
                      }}
                    >
                      <ChevronDown
                        size={12}
                        aria-hidden
                        className={expandedIds.has(notif.id) ? "rotate-180" : ""}
                      />
                      {expandedIds.has(notif.id) ? "Hide message" : "Show message"}
                    </button>
                    {expandedIds.has(notif.id) && (
                      <div className="mt-1 whitespace-pre-wrap break-words text-[12px] leading-5 text-[var(--ct-muted-fg)]">
                        {notif.body}
                      </div>
                    )}
                  </>
                )}
                {notif.tab_id && (
                  <button
                    type="button"
                    className="mt-1.5 rounded border border-[var(--ct-border)] px-2 py-1 text-[10px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                    onClick={(event) => {
                      event.stopPropagation();
                      if (!notif.read) onMarkRead(notif.id);
                      onJumpToTab(notif.tab_id!, notif.workspace_id);
                    }}
                  >
                    Show Tab
                  </button>
                )}
              </div>
              <button
                type="button"
                aria-label={`Delete ${notif.title}`}
                title="Delete"
                className="mt-0.5 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted)] opacity-0 hover:bg-[var(--ct-hover)] hover:text-[var(--ct-red)] group-hover:opacity-100 focus:opacity-100"
                onClick={(event) => {
                  event.stopPropagation();
                  onDelete(notif.id);
                }}
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function LevelDot({ level }: { level: Notification["level"] }) {
  return (
    <span
      className="inline-block w-2 h-2 rounded-full shrink-0 mt-1"
      style={{ backgroundColor: LEVEL_COLORS[level] }}
    />
  );
}

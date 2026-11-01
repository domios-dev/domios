import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useEffect, useRef, useState } from "react";
import { NotificationBody } from "./NotificationBody";
import type { Notification } from "../../lib/types";
import { neighborNotificationTarget, runNotificationAction } from "../../lib/notificationActions";
import { Bell, Check, ChevronDown, Trash2, X } from "lucide-react";
import { timeAgo } from "../../lib/formatters";
import { isAnswerUndeliverable, isAnswerUndelivered } from "../../lib/notificationAnswer";
import { notificationLevel, notificationTone } from "../../lib/notifyLevels";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { Skeleton, SkeletonBlock } from "../shared/Skeleton";

const NOTIFICATION_SKELETON_ROWS = [78, 64, 70];

interface NotificationPanelProps {
  questionId?: string;
  notifications: Notification[];
  /** History has not answered yet: an empty list means unknown, not none. */
  loading?: boolean;
  onMarkRead: (id: string) => void;
  onMarkAllRead: () => void;
  onDelete: (id: string) => void;
  onClearAll: () => void;
  onJumpToTab: (tabId: string, workspaceId: string | null) => void;
  onAnswer?: (id: string, label: string) => void;
  onClose: () => void;
}

export function NotificationPanel({
  questionId,
  notifications,
  loading = false,
  onMarkRead,
  onMarkAllRead,
  onDelete,
  onClearAll,
  onJumpToTab,
  onAnswer,
  onClose,
}: NotificationPanelProps) {
  const sorted = [...notifications].sort((a, b) => b.timestamp - a.timestamp);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => {
    const newestWithBody = sorted.find((notification) => notification.body);
    return newestWithBody ? new Set([newestWithBody.id]) : new Set();
  });
  const pendingId = questionId ?? sorted.find(n => !n.resolved && n.answer && !n.answer.answered)?.id;
  const pendingRow = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (pendingId) {
      setExpandedIds(ids => new Set([...ids, pendingId]));
      pendingRow.current?.scrollIntoView?.({ block: "nearest" });
    }
  }, [pendingId]);

  const handleClick = useCallback(
    (notif: Notification) => {
      if (!notif.read) onMarkRead(notif.id);
      const neighbor = notif.actions?.find((action) => action.type === "url" && action.value && neighborNotificationTarget(action.value));
      if (neighbor) void runNotificationAction(neighbor);
      else if (notif.tab_id) onJumpToTab(notif.tab_id, notif.workspace_id);
    },
    [onJumpToTab, onMarkRead],
  );
  const headerBtnClass = `${UI_BUTTON.ghost} ${UI_BUTTON.compact} h-6 shrink-0 whitespace-nowrap px-1.5 text-[10px] disabled:pointer-events-none disabled:opacity-35`;
  const rowClass = "ct-row group flex items-start gap-2 px-2 py-2.5";
  // The card cannot widen past the panel it lives in, so reading room is the
  // panel's to give: it grows while anything is open and returns when nothing is.
  const anyExpanded = expandedIds.size > 0;

  return (
    <div
      data-testid="notification-panel"
      data-expanded={anyExpanded ? "true" : "false"}
      className="flex max-w-full flex-col overflow-hidden rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] shadow-2xl ct-float"
      style={{
        // A plain width. `max-w-full` hands the clamp to the anchored wrapper,
        // which knows how much room exists to the left of the bell.
        width: anyExpanded ? "560px" : "360px",
        transition: "width var(--ct-duration-normal) var(--ct-ease-out-expo)",
        maxHeight: "min(520px, calc(100dvh - var(--ct-safe-top, 0px) - var(--ct-safe-bottom, 0px) - 48px))",
      }}
    >
      <div className="flex min-h-9 shrink-0 flex-wrap items-center justify-between gap-x-2 px-2.5 py-1">
        <span className="flex items-center gap-1.5 text-[11px] font-medium text-[var(--ct-fg)]">
          <Bell size={13} /> Notifications
        </span>
        <div className="ml-auto flex shrink-0 items-center gap-1">
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
      <div className="ct-rows flex-1 overflow-y-auto">
        {sorted.length === 0 && loading ? (
          <Skeleton data-testid="notification-skeleton" className="flex flex-col">
            {NOTIFICATION_SKELETON_ROWS.map((width) => (
              <div key={width} className="flex items-start gap-2 px-2 py-2.5">
                <SkeletonBlock shape="circle" width={8} height={8} className="mt-1" />
                <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                  <SkeletonBlock width={`${width}%`} height={10} />
                  <SkeletonBlock width={`${width - 25}%`} height={8} faint />
                </div>
              </div>
            ))}
          </Skeleton>
        ) : sorted.length === 0 ? (
          <div className="flex h-28 flex-col items-center justify-center gap-2">
            <Bell size={18} className="text-[var(--ct-muted)]" />
            <span className={UI_TEXT.metaMuted}>No notifications</span>
          </div>
        ) : (
          sorted.map((notif) => (
            <div
              key={notif.id}
              data-testid="notification-row"
              data-notification-id={notif.id}
              ref={notif.id === pendingId ? pendingRow : undefined}
              onClick={() => handleClick(notif)}
              className={rowClass}
            >
              <LevelDot notification={notif} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-2">
                  <span
                    className={`truncate text-[11px] font-medium ${notif.read ? "text-[var(--ct-muted-fg)]" : "text-[var(--ct-fg)]"}`}
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
                      <div
                        data-testid="notification-expanded-body"
                        role="region"
                        tabIndex={0}
                        aria-label={`Message from ${notif.title}`}
                        className="mt-1.5 break-words rounded border border-[var(--ct-border)] px-2 py-1.5 text-[12px] leading-5 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--ct-accent)]"
                        style={{
                          color: "var(--ct-fg)",
                          backgroundColor: "var(--ct-bg)",
                          maxHeight: "min(14rem, 40dvh)",
                          overflowY: "auto",
                          overscrollBehavior: "contain",
                        }}
                      >
                        <NotificationBody text={notif.body} tabId={notif.tab_id} failure={notif.level === "failure" || notif.severity === "Error"} operation={notif.title} />
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
                {notif.local_actions?.map((action) => <button key={action.label} type="button" className="mt-1.5 mr-2 text-xs text-[var(--ct-accent)]" onClick={(event) => { event.stopPropagation(); void action.run(); }}>{action.label}</button>)}
                {notif.answer_pending && <div role="status">Sending answer…</div>}
                {notif.answer_error && <InlineErrorNotice error={notif.answer_error} operation="answering this notification" blocking />}
                {notif.resolved ? (
                  <div
                    data-testid="notification-resolved"
                    className="mt-2 text-[11px] text-[var(--ct-muted-fg)]"
                  >
                    Resolved in tab.
                  </div>
                ) : notif.answer?.answered && !notif.answer_error && !notif.answer_pending ? (
                  isAnswerUndelivered(notif.answer) ? (
                    <div
                      data-testid="notification-answer-undelivered"
                      className="mt-2 text-[11px] text-[var(--ct-yellow)]"
                    >
                      You answered: {notif.answer.answered} — it could not be delivered to
                      the tab that asked.
                    </div>
                  ) : (
                    <div
                      data-testid="notification-answered"
                      className="mt-2 inline-flex items-center gap-1 text-[11px] text-[var(--ct-green)]"
                    >
                      <Check size={12} aria-hidden />
                      <span>You answered: {notif.answer.answered}</span>
                    </div>
                  )
                ) : notif.answer && isAnswerUndeliverable(notif.answer) ? (
                  <div
                    data-testid="notification-undeliverable"
                    className="mt-2 text-[11px] text-[var(--ct-muted-fg)]"
                  >
                    The tab that asked is gone — this can no longer be answered.
                  </div>
                ) : (
                  (notif.actions?.length ?? 0) > 0 && (
                    <div className="mt-2 flex flex-wrap gap-1.5">
                      {notif.actions!.map((action, index) => (
                        <button
                          type="button"
                          key={`${action.label}-${index}`}
                          disabled={notif.answer_pending || (notif.answer?.answered != null && notif.answer.answered !== action.label)}
                          className={
                            (action.type === "answer" || action.type === "respond_prompt_option") && index === 0
                              ? "rounded border border-[var(--ct-accent)] px-2 py-1 text-[10px] text-[var(--ct-accent)] hover:bg-[var(--ct-hover)]"
                              : "rounded border border-[var(--ct-border)] px-2 py-1 text-[10px] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
                          }
                          onClick={(event) => {
                            event.stopPropagation();
                            if (action.type === "answer" || action.type === "respond_prompt_option") {
                              onAnswer?.(notif.id, action.label);
                            }
                            else void runNotificationAction(action);
                          }}
                        >
                          {action.label}
                        </button>
                      ))}
                    </div>
                  )
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

function LevelDot({ notification }: { notification: Notification }) {
  return (
    <span
      data-testid="notification-level-dot"
      data-notification-level={notificationLevel(notification) ?? ""}
      aria-hidden
      className="inline-block w-2 h-2 rounded-full shrink-0 mt-1"
      style={{ backgroundColor: notificationTone(notification) }}
    />
  );
}

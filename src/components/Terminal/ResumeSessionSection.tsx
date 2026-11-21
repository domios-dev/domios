import { useEffect, useState } from "react";
import { RotateCcw } from "lucide-react";
import {
  formatSessionAge,
  listResumableSessions,
  resumeListedSession,
  type ResumableSessionItem,
  type ResumableSessionsTransport,
} from "../../lib/resumableSessions";

export function ResumeSessionSection({
  workspaceId,
  cwd,
  disabled,
  providerLabel,
  onResumed,
  onError,
  transport,
}: {
  workspaceId: string;
  cwd: string | null | undefined;
  disabled: boolean;
  providerLabel: (providerId: string) => string;
  onResumed: (result: { workspaceId: string; tabId?: string }) => void;
  onError: (error: unknown) => void;
  transport?: ResumableSessionsTransport;
}) {
  const [sessions, setSessions] = useState<ResumableSessionItem[]>([]);
  const [resumingId, setResumingId] = useState<string | null>(null);

  useEffect(() => {
    const targetCwd = cwd?.trim();
    if (!targetCwd) {
      setSessions([]);
      return;
    }
    let cancelled = false;
    void listResumableSessions(targetCwd, 5, transport)
      .then((items) => {
        if (!cancelled) setSessions(items);
      })
      .catch(() => {
        if (!cancelled) setSessions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [cwd, transport]);

  if (sessions.length === 0) return null;

  const handleResume = async (item: ResumableSessionItem) => {
    if (disabled || resumingId) return;
    setResumingId(item.sessionId);
    try {
      const result = await resumeListedSession(item.sessionId, workspaceId, transport);
      onResumed({ workspaceId: result.workspaceId ?? workspaceId, tabId: result.tabId });
    } catch (error) {
      onError(error);
    } finally {
      setResumingId(null);
    }
  };

  return (
    <div
      data-testid="picker-resume-sessions"
      className="mt-3 border-t pt-3"
      style={{ borderColor: "var(--ct-border)" }}
    >
      <div className="mb-1.5 text-[11px] font-medium" style={{ color: "var(--ct-muted-fg)" }}>
        Resume recent
      </div>
      <div className="flex flex-col gap-1">
        {sessions.map((item) => {
          const age = formatSessionAge(item.updatedAtSec);
          const busy = resumingId === item.sessionId;
          return (
            <button
              key={`${item.provider}:${item.sessionId}`}
              type="button"
              data-testid="picker-resume-session-item"
              disabled={disabled || resumingId !== null}
              onClick={() => void handleResume(item)}
              className="flex w-full min-w-0 items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors hover:bg-[var(--ct-hover)] disabled:opacity-50"
              style={{ color: "var(--ct-fg)" }}
            >
              <span className="flex min-w-0 items-center gap-2">
                <RotateCcw aria-hidden="true" size={12} className="shrink-0" style={{ color: "var(--ct-muted-fg)" }} />
                <span className="truncate">{item.title}</span>
              </span>
              <span className="flex shrink-0 items-center gap-2 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
                <span>{providerLabel(item.provider)}</span>
                {age && <span>Updated {age}</span>}
                {busy && <span>Resuming…</span>}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

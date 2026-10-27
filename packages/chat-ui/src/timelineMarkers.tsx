import { createContext, useContext, useState, type ReactNode } from "react";
import type {
  ChatSessionIntent,
  ChatTimelineMarker,
  NormalizedChatMessage as ChatMessage,
} from "@codeterm/chat-core";

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function cleanIntentText(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const text = value.trim();
  return text && !/[\u0000-\u001f\u007f]/.test(text) ? text : undefined;
}

export function normalizedTimelineMarker(message: ChatMessage): ChatTimelineMarker | null {
  const marker: unknown = message.timelineMarker;
  if (!isRecord(marker) || typeof marker.kind !== "string") return null;
  if (marker.kind === "queued") return { kind: "queued" };
  if (marker.kind === "context_compaction") return { kind: "context_compaction" };
  if (marker.kind === "external_edit") {
    const path = cleanIntentText(marker.path);
    return path ? { kind: "external_edit", path } : null;
  }
  if (marker.kind === "history_gap") {
    const sessionId = cleanIntentText(marker.session_id);
    const source = marker.source === "provider_session" || marker.source === "chat_backend" ? marker.source : null;
    const before = marker.before_offset;
    if (!sessionId || !source || typeof before !== "number" || !Number.isSafeInteger(before) || before <= 0) return null;
    return { kind: "history_gap", source, session_id: sessionId, before_offset: before };
  }
  return null;
}

type HistoryGapLoader = (message: ChatMessage) => Promise<void>;

const HistoryGapContext = createContext<{ load?: HistoryGapLoader; label?: (message: ChatMessage) => string }>({});

/** Supplies the loader a history-gap row calls to page the older history behind it. */
export function HistoryGapProvider({ onLoad, label, children }: { onLoad?: HistoryGapLoader; label?: (message: ChatMessage) => string; children: ReactNode }) {
  return <HistoryGapContext.Provider value={{ load: onLoad, label }}>{children}</HistoryGapContext.Provider>;
}

function HistoryGapRow({ message }: { message: ChatMessage }): ReactNode {
  const { load, label } = useContext(HistoryGapContext);
  const [state, setState] = useState<"idle" | "loading" | "failed">("idle");
  const loadEarlier = () => {
    if (!load || state === "loading") return;
    setState("loading");
    load(message).then(() => setState("idle"), () => setState("failed"));
  };
  return (
    <div
      role="group"
      aria-label="Earlier history not loaded"
      data-testid="chat-history-gap"
      className="mb-3 flex w-full max-w-4xl min-w-0 items-center gap-3 mx-auto"
      style={{ color: "var(--ct-muted-fg)" }}
    >
      <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
      <span className="text-center text-[11px]">
        {state === "failed" ? "Earlier history could not be loaded." : message.content}
      </span>
      {load ? (
        <button
          type="button"
          onClick={loadEarlier}
          disabled={state === "loading"}
          className="rounded px-2 py-0.5 text-[11px]"
          style={{ border: "1px solid var(--ct-border)", color: "var(--ct-fg)" }}
        >
          {state === "loading" ? "Loading…" : state === "failed" ? "Retry" : label?.(message) ?? "Load previous messages (up to 100)"}
        </button>
      ) : null}
      <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
    </div>
  );
}

export function sessionIntentFromMessages(
  messages: readonly ChatMessage[],
): ChatSessionIntent | null {
  let title: string | undefined;
  let goal: string | undefined;
  let goalStatus: string | undefined;
  for (const message of messages) {
    const intent: unknown = message.sessionIntent;
    if (!isRecord(intent)) continue;
    const nextTitle = cleanIntentText(intent.title);
    const nextGoal = cleanIntentText(intent.goal);
    if (nextTitle) title = nextTitle;
    if (nextGoal) {
      goal = nextGoal;
      goalStatus = cleanIntentText(intent.goalStatus);
    } else if (Object.keys(intent).length === 0 || intent.goal === null) {
      goal = undefined;
      goalStatus = undefined;
    }
  }
  if (!title && !goal) return null;
  return { ...(title ? { title } : {}), ...(goal ? { goal } : {}), ...(goalStatus ? { goalStatus } : {}) };
}

export function isSessionIntentOnlyMessage(message: ChatMessage): boolean {
  return message.type === "custom"
    && !message.content.trim()
    && isRecord(message.sessionIntent)
    && normalizedTimelineMarker(message) === null;
}

export function TimelineMarkerRow({ message }: { message: ChatMessage }): ReactNode {
  const marker = normalizedTimelineMarker(message);
  if (!marker || marker.kind === "queued") return null;
  if (marker.kind === "history_gap") return <HistoryGapRow message={message} />;
  if (marker.kind === "context_compaction") {
    const compaction = message.contextCompaction;
    const running = compaction?.state === "running";
    const failed = compaction?.state === "failed";
    const label = running ? "Compacting context…" : failed ? "Context compaction failed" : "Context compacted";
    const duration = compaction?.endedAt == null ? null : Math.max(0, compaction.endedAt - compaction.startedAt);
    return (
      <div
        role={running || failed ? "status" : "separator"}
        aria-label={label}
        aria-busy={running || undefined}
        data-testid="chat-context-compacted-divider"
        className="mb-3 flex w-full max-w-4xl min-w-0 items-center gap-3 mx-auto"
        style={{ color: failed ? "var(--ct-err)" : "var(--ct-muted-fg)" }}
      >
        <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
        <span className="inline-flex max-w-[70%] items-center gap-2 text-center text-[11px]">
          {running && <svg aria-hidden="true" viewBox="0 0 16 16" width="12" height="12" className="motion-safe:animate-spin"><circle cx="8" cy="8" r="5.5" fill="none" stroke="currentColor" strokeWidth="1.5" opacity="0.2" /><path d="M8 2.5a5.5 5.5 0 0 1 5.5 5.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" /></svg>}
          {label}
          {duration != null && <span className="tabular-nums opacity-70">{(duration / 1000).toFixed(1)}s</span>}
        </span>
        <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
      </div>
    );
  }
  const label = `changed outside: ${marker.path}`;
  return (
    <div
      role="status"
      aria-label={label}
      data-testid="chat-external-edit-row"
      className="mb-2 w-full max-w-4xl min-w-0 mx-auto"
    >
      <div
        className="inline-flex max-w-full min-w-0 rounded-md px-2.5 py-1 text-xs"
        style={{
          background: "var(--ct-surface)",
          color: "var(--ct-muted-fg)",
        }}
      >
        {label}
      </div>
    </div>
  );
}

export function ChatSessionIntentLine({ intent }: { intent: ChatSessionIntent | null }): ReactNode {
  const goal = cleanIntentText(intent?.goal);
  if (!goal) return null;
  return (
    <div
      data-testid="chat-session-intent"
      role="group"
      aria-label="Session intent"
      className="shrink-0 border-b px-3 py-1.5"
      style={{
        background: "var(--ct-surface)",
        borderColor: "var(--ct-border)",
        color: "var(--ct-muted-fg)",
      }}
    >
      <div className="mx-auto flex w-full max-w-4xl flex-wrap items-baseline gap-x-3 gap-y-0.5 text-[11px]">
        <span data-testid="chat-session-goal" className="min-w-0">
          <span className="mr-1 font-mono" style={{ color: "var(--ct-accent)" }}>goal</span>
          <span style={{ color: "var(--ct-fg)" }}>{goal}</span>
        </span>
      </div>
    </div>
  );
}

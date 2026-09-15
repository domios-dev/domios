import type { ReactNode } from "react";
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
  return null;
}

export function sessionIntentFromMessages(
  messages: readonly ChatMessage[],
): ChatSessionIntent | null {
  let title: string | undefined;
  let goal: string | undefined;
  for (const message of messages) {
    const intent: unknown = message.sessionIntent;
    if (!isRecord(intent)) continue;
    const nextTitle = cleanIntentText(intent.title);
    const nextGoal = cleanIntentText(intent.goal);
    if (nextTitle) title = nextTitle;
    if (nextGoal) goal = nextGoal;
  }
  if (!title && !goal) return null;
  return { ...(title ? { title } : {}), ...(goal ? { goal } : {}) };
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
  if (marker.kind === "context_compaction") {
    return (
      <div
        role="separator"
        aria-label="context compacted"
        data-testid="chat-context-compacted-divider"
        className="mb-3 flex w-full max-w-4xl min-w-0 items-center gap-3 mx-auto"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
        <span className="max-w-[70%] text-center text-[11px]">context compacted</span>
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

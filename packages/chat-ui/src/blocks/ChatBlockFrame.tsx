import React, { useId, useRef, useState } from "react";
import { ChevronRight, Loader2, MessageSquarePlus, Pin } from "lucide-react";
import { ChatCardMenu } from "../ChatCardMenu";
import { useChatBlockHost, type ChatCardMenuItem } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";

export type ChatBlockTone = "ok" | "accent" | "warn" | "muted";

export interface ChatBlockStatus {
  label: string;
  tone: ChatBlockTone;
}

/** A user-click action. `run` resolves to the result line shown in place; a thrown error becomes the failure line. */
export interface ChatBlockAction {
  id: string;
  label: string;
  ariaLabel?: string;
  busyLabel?: string;
  primary?: boolean;
  disabled?: boolean;
  reason?: string;
  confirm?: { message: string; confirmLabel: string };
  run(): Promise<string | void> | string | void;
}

export interface ChatBlockFrameProps {
  name: string;
  source: string;
  title: string;
  icon: React.ReactNode;
  accent?: string | null;
  status?: ChatBlockStatus;
  summary?: React.ReactNode;
  /** Always-visible content between the header and the actions row. */
  body?: React.ReactNode;
  details?: React.ReactNode;
  actions?: readonly ChatBlockAction[];
  fullPage?: { label: string; open(): void };
  askText?: string;
  muted?: boolean;
  /** False for one-shot blocks whose state belongs to their message, such as a choice. */
  pinnable?: boolean;
}

type Outcome = { kind: "idle" } | { kind: "running"; id: string } | { kind: "done"; message: string } | { kind: "failed"; message: string };

export const TONE_COLOR: Record<ChatBlockTone, string> = {
  ok: "var(--ct-ok)",
  accent: "var(--ct-accent)",
  warn: "var(--ct-warn)",
  muted: "var(--ct-muted-fg)",
};

export const FOCUS_RING = "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]";

/** A compact field that sits on the block surface. */
export const BLOCK_INPUT = "ct-field";

/** The shared chrome every chat block renders in: header, inline body, expandable details, actions and extras. */
export function ChatBlockFrame({
  name, source, title, icon, accent, status, summary, body, details, actions = [], fullPage, askText, muted = false, pinnable = true,
}: ChatBlockFrameProps) {
  const host = useChatBlockHost();
  const pinned = useLiveBinding(host.pinned, source, false);
  const [expanded, setExpanded] = useState(false);
  const [outcome, setOutcome] = useState<Outcome>({ kind: "idle" });
  const [confirming, setConfirming] = useState<ChatBlockAction | null>(null);
  const running = useRef(false);
  const detailsId = useId();
  const tint = accent ?? "var(--ct-accent)";

  const execute = async (action: ChatBlockAction) => {
    if (running.current || action.disabled) return;
    running.current = true;
    setConfirming(null);
    setOutcome({ kind: "running", id: action.id });
    try {
      const message = await action.run();
      setOutcome(message ? { kind: "done", message } : { kind: "idle" });
    } catch (error) {
      setOutcome({ kind: "failed", message: `${action.label} failed: ${error instanceof Error ? error.message : String(error)}` });
    } finally {
      running.current = false;
    }
  };
  const onAction = (action: ChatBlockAction) => (action.confirm ? setConfirming(action) : void execute(action));
  const busy = outcome.kind === "running";
  const hasFooter = confirming !== null || actions.length > 0 || fullPage !== undefined || details !== undefined || !body;

  return (
    <section
      data-testid="chat-block"
      data-chat-block={name}
      aria-label={title}
      className={`ct-card my-2 w-full max-w-[34rem] min-w-0${muted ? " ct-card--quiet" : ""}`}
      style={{ "--ct-card-tone": tint, opacity: muted ? 0.78 : 1 } as React.CSSProperties}
    >
      <header className="ct-card__head" style={{ alignItems: "flex-start" }}>
        <span aria-hidden="true" className="ct-card__icon">{icon}</span>
        <div className="min-w-0 flex-1 pt-0.5">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
            <h4 className="ct-card__title m-0 min-w-0 truncate" title={title}>{title}</h4>
            {status && (
              <span data-testid="chat-block-status" data-tone={status.tone} className="ct-card__meta inline-flex items-center gap-1.5" style={{ color: TONE_COLOR[status.tone] }}>
                <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full" style={{ background: "currentColor" }} />
                {status.label}
              </span>
            )}
            {pinned && <Pin size={11} aria-label="Pinned to the side panel" className="shrink-0" style={{ color: "var(--ct-muted-fg)" }} />}
          </div>
          {summary && <div className="ct-card__meta mt-0.5 leading-relaxed">{summary}</div>}
        </div>
        <BlockMenu source={source} title={title} askText={askText} pinned={pinned} pinnable={pinnable} />
      </header>

      {body && <div data-testid="chat-block-body" className="ct-card__body" style={hasFooter ? { paddingBottom: 0 } : undefined}>{body}</div>}

      {details && expanded && (
        <div id={detailsId} data-testid="chat-block-details" className="ct-card__body ct-card__body--reveal mt-2" style={{ paddingBottom: 0, color: "var(--ct-muted-fg)" }}>
          {details}
        </div>
      )}

      {hasFooter && <div className="ct-card__body flex flex-wrap items-center gap-2" style={{ paddingTop: 10 }}>
        {confirming ? (
          <div role="group" aria-label={`Confirm ${confirming.label}`} className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
            <span className="min-w-0 flex-1">{confirming.confirm?.message}</span>
            <BlockButton primary onClick={() => void execute(confirming)}>{confirming.confirm?.confirmLabel}</BlockButton>
            <BlockButton onClick={() => setConfirming(null)}>Cancel</BlockButton>
          </div>
        ) : (
          <>
            {actions.map((action) => {
              const isRunning = outcome.kind === "running" && outcome.id === action.id;
              return (
                <BlockButton
                  key={action.id}
                  primary={action.primary}
                  disabled={action.disabled || busy}
                  title={action.reason}
                  data-block-action={action.id}
                  aria-label={action.ariaLabel}
                  aria-busy={isRunning || undefined}
                  onClick={() => onAction(action)}
                >
                  {isRunning && <Loader2 size={12} aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
                  {isRunning ? action.busyLabel ?? action.label : action.label}
                </BlockButton>
              );
            })}
            {fullPage && <BlockButton onClick={fullPage.open}>{fullPage.label}</BlockButton>}
            {details && (
              <button
                type="button"
                aria-expanded={expanded}
                aria-controls={detailsId}
                onClick={() => setExpanded((open) => !open)}
                className={`ml-auto inline-flex items-center gap-1 rounded-md px-1 text-xs hover:text-[var(--ct-fg)] ${FOCUS_RING}`}
                style={{ color: "var(--ct-muted-fg)" }}
              >
                Details
                <ChevronRight size={12} aria-hidden="true" className={`transition-transform motion-reduce:transition-none ${expanded ? "rotate-90" : ""}`} />
              </button>
            )}
          </>
        )}
      </div>}

      <div aria-live="polite" className="ct-card__body empty:hidden" data-testid="chat-block-result">
        {outcome.kind === "done" && <span style={{ color: TONE_COLOR.ok }}>{outcome.message}</span>}
        {outcome.kind === "failed" && <span role="alert" style={{ color: "var(--ct-err)" }}>{outcome.message}</span>}
      </div>
    </section>
  );
}

export function BlockButton({ primary = false, children, className = "", ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { primary?: boolean; "data-block-action"?: string }) {
  return (
    <button
      type="button"
      {...props}
      className={`ct-bar-pill${primary ? " ct-bar-pill--primary" : ""} disabled:opacity-50 ${className}`}
    >
      {children}
    </button>
  );
}

function BlockMenu({ source, title, askText, pinned, pinnable }: { source: string; title: string; askText?: string; pinned: boolean; pinnable: boolean }) {
  const host = useChatBlockHost();
  const items: ChatCardMenuItem[] = [];
  if (host.askAbout) {
    const askAbout = host.askAbout;
    items.push({
      label: "Ask about this",
      icon: <MessageSquarePlus size={14} aria-hidden="true" />,
      testId: "chat-block-ask",
      onSelect: () => askAbout({ title, text: askText ?? `About ${title} (\`${source}\`)` }),
    });
  }
  if (host.pin && pinnable) {
    const pin = host.pin;
    items.push({
      label: "Pin to side panel",
      icon: <Pin size={14} aria-hidden="true" />,
      checked: pinned,
      testId: "chat-block-pin",
      onSelect: () => pin(source),
    });
  }
  return <ChatCardMenu label={`Actions for ${title}`} items={items} />;
}

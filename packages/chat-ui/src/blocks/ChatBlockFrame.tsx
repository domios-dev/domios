import React, { useId, useRef, useState } from "react";
import { ChevronRight, Loader2, MessageSquarePlus, Pin } from "lucide-react";
import { useChatBlockHost } from "./ChatBlockHost";
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
  ok: "var(--ct-ok, #3fb950)",
  accent: "var(--ct-accent, #58a6ff)",
  warn: "var(--ct-warn, #d29922)",
  muted: "var(--ct-muted-fg, #8b949e)",
};

const FOCUS_RING = "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]";

/** The shared chrome every chat block renders in: header, compact body, expandable details, actions and extras. */
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
  const tint = accent ?? "var(--ct-accent, #58a6ff)";

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
      className="my-2 w-full max-w-[34rem] min-w-0 rounded-xl border"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 94%, var(--ct-fg, #c9d1d9) 6%)",
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
        color: "var(--ct-fg)",
        opacity: muted ? 0.78 : 1,
      }}
    >
      <header className="flex min-w-0 items-start gap-3 px-3 pt-3">
        <span
          aria-hidden="true"
          className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[10px]"
          style={{
            background: `color-mix(in srgb, ${tint} ${muted ? 8 : 16}%, transparent)`,
            boxShadow: `inset 0 0 0 1px color-mix(in srgb, ${tint} ${muted ? 18 : 34}%, transparent)`,
            color: tint,
          }}
        >
          {icon}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
            <h4 className="m-0 min-w-0 truncate text-[13px] font-semibold leading-5" title={title}>{title}</h4>
            {status && (
              <span data-testid="chat-block-status" data-tone={status.tone} className="inline-flex items-center gap-1.5 text-[11px] leading-5" style={{ color: TONE_COLOR[status.tone] }}>
                <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full" style={{ background: "currentColor" }} />
                {status.label}
              </span>
            )}
          </div>
          {summary && <div className="mt-0.5 text-xs leading-relaxed" style={{ color: "var(--ct-muted-fg)" }}>{summary}</div>}
        </div>
        <BlockExtras source={source} title={title} askText={askText} pinned={pinned} pinnable={pinnable} />
      </header>

      {body && <div data-testid="chat-block-body" className={`mt-2 min-w-0 px-3 ${hasFooter ? "" : "pb-3"}`}>{body}</div>}

      {details && expanded && (
        <div id={detailsId} data-testid="chat-block-details" className="mx-3 mt-2 border-t pt-2 text-xs leading-relaxed" style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 18%, transparent)", color: "var(--ct-muted-fg)" }}>
          {details}
        </div>
      )}

      {hasFooter && <div className="flex min-w-0 flex-wrap items-center gap-2 px-3 pb-3 pt-2.5">
        {confirming ? (
          <div role="group" aria-label={`Confirm ${confirming.label}`} className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
            <span className="min-w-0 flex-1 text-xs">{confirming.confirm?.message}</span>
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
                className={`ml-auto inline-flex items-center gap-1 rounded px-1 text-xs hover:text-[var(--ct-fg)] ${FOCUS_RING}`}
                style={{ color: "var(--ct-muted-fg)" }}
              >
                Details
                <ChevronRight size={12} aria-hidden="true" className={`transition-transform motion-reduce:transition-none ${expanded ? "rotate-90" : ""}`} />
              </button>
            )}
          </>
        )}
      </div>}

      <div aria-live="polite" className="empty:hidden px-3 pb-3 -mt-1 text-xs" data-testid="chat-block-result">
        {outcome.kind === "done" && <span style={{ color: TONE_COLOR.ok }}>{outcome.message}</span>}
        {outcome.kind === "failed" && <span role="alert" style={{ color: "var(--ct-err, #f85149)" }}>{outcome.message}</span>}
      </div>
    </section>
  );
}

function BlockButton({ primary = false, children, className = "", ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { primary?: boolean; "data-block-action"?: string }) {
  return (
    <button
      type="button"
      {...props}
      className={`inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium transition-[filter,background-color] disabled:cursor-default disabled:opacity-50 motion-reduce:transition-none ${primary ? "hover:enabled:brightness-110" : "border hover:enabled:bg-[var(--ct-hover,rgba(127,127,127,0.12))]"} ${FOCUS_RING} ${className}`}
      style={primary
        ? { background: "var(--ct-accent, #58a6ff)", color: "var(--ct-on-accent, #fff)" }
        : { borderColor: "color-mix(in srgb, var(--ct-muted-fg) 30%, transparent)", color: "var(--ct-fg)" }}
    >
      {children}
    </button>
  );
}

function BlockExtras({ source, title, askText, pinned, pinnable }: { source: string; title: string; askText?: string; pinned: boolean; pinnable: boolean }) {
  const host = useChatBlockHost();
  if (!host.askAbout && !(host.pin && pinnable)) return null;
  return (
    <div className="-mr-1 flex shrink-0 items-center" data-testid="chat-block-extras">
      {host.askAbout && (
        <ExtraButton label={`Ask about ${title}`} onClick={() => host.askAbout?.(askText ?? `About ${title} (\`${source}\`): `)}>
          <MessageSquarePlus size={13} aria-hidden="true" />
        </ExtraButton>
      )}
      {host.pin && pinnable && (
        <ExtraButton label={pinned ? `Unpin ${title} from the side panel` : `Pin ${title} to the side panel`} pressed={pinned} onClick={() => host.pin?.(source)}>
          <Pin size={13} aria-hidden="true" fill={pinned ? "currentColor" : "none"} />
        </ExtraButton>
      )}
    </div>
  );
}

function ExtraButton({ label, pressed, children, ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string; pressed?: boolean }) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={pressed}
      title={label}
      {...props}
      className={`flex h-6 w-6 items-center justify-center rounded-md hover:bg-[var(--ct-hover,rgba(127,127,127,0.12))] hover:text-[var(--ct-fg)] ${FOCUS_RING}`}
      style={{ color: pressed ? "var(--ct-accent)" : "var(--ct-muted-fg)" }}
    >
      {children}
    </button>
  );
}

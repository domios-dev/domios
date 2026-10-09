import { useRef, useState } from "react";
import { Check, Loader2, MessageCircleQuestionMark } from "lucide-react";
import { ChatBlockFrame, type ChatBlockStatus } from "./ChatBlockFrame";
import { useChatBlockHost, useChatBlockMessageId } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";
import { choiceOutcome, type ChoiceOutcome } from "./choiceBlockModel";


const STATUS: Record<ChoiceOutcome["kind"], ChatBlockStatus> = {
  open: { label: "Waiting for you", tone: "warn" },
  sending: { label: "Sending", tone: "accent" },
  chosen: { label: "Sent", tone: "ok" },
  answered: { label: "Answered in chat", tone: "muted" },
  unavailable: { label: "Answer in the agent's chat", tone: "muted" },
};

function summaryFor(outcome: ChoiceOutcome): string {
  switch (outcome.kind) {
    case "open": return "Tap an answer to send it as your reply.";
    case "sending": return `Sending “${outcome.label}”…`;
    case "chosen": return `You replied “${outcome.label}”.`;
    case "answered": return "You replied in the chat instead.";
    case "unavailable": return "These answers can be sent only from the chat they were asked in.";
  }
}

/** `@ds-choice A | B`: tappable answers; one tap sends that label once as the user's reply in this chat. */
export function ChoiceBlock({ options, source }: { options: string[]; source: string }) {
  const host = useChatBlockHost().choice;
  const messageId = useChatBlockMessageId();
  const bound = host && messageId ? host : undefined;
  const state = useLiveBinding(bound?.binding, messageId ?? "", null);
  const [sending, setSending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const claimed = useRef(false);
  const outcome = choiceOutcome(options, bound ? state : null, sending);
  const open = outcome.kind === "open";

  const choose = async (label: string) => {
    if (!bound || !messageId || claimed.current || !open) return;
    claimed.current = true;
    setError(null);
    setSending(label);
    try {
      if (!(await bound.choose(messageId, label))) setError("This question was already answered.");
    } catch (failure) {
      claimed.current = false;
      setSending(null);
      setError(`Could not send “${label}”: ${failure instanceof Error ? failure.message : String(failure)}`);
    }
  };

  const picked = outcome.kind === "chosen" || outcome.kind === "sending" ? outcome.label : null;
  return (
    <ChatBlockFrame
      name="choice"
      source={source}
      title="Choose a reply"
      icon={<MessageCircleQuestionMark size={14} aria-hidden="true" />}
      status={STATUS[outcome.kind]}
      summary={summaryFor(outcome)}
      pinnable={false}
      muted={outcome.kind === "answered" || outcome.kind === "unavailable"}
      body={(
        <>
          <div role="group" aria-label="Answers" className="flex min-w-0 flex-wrap gap-1.5">
            {options.map((label) => {
              const isPicked = label === picked;
              return (
                <button
                  key={label}
                  type="button"
                  data-choice-option={label}
                  aria-pressed={isPicked}
                  disabled={!open}
                  onClick={() => void choose(label)}
                  className={`ct-bar-pill max-w-full text-left disabled:cursor-default${isPicked ? " ct-bar-pill--primary" : ""} ${!open && !isPicked ? "opacity-45" : ""}`}
                  style={{ height: "auto", minHeight: 28, paddingBlock: 4, whiteSpace: "normal" }}
                >
                  {isPicked && (outcome.kind === "sending"
                    ? <Loader2 size={12} aria-hidden="true" className="shrink-0 animate-spin motion-reduce:animate-none" />
                    : <Check size={12} aria-hidden="true" className="shrink-0" style={{ color: "var(--ct-accent)" }} />)}
                  <span className="min-w-0 break-words">{label}</span>
                </button>
              );
            })}
          </div>
          {error && <p role="alert" className="m-0 mt-2 text-xs" style={{ color: "var(--ct-err)" }}>{error}</p>}
        </>
      )}
      askText={`About this question (\`${source}\`): `}
    />
  );
}

import { useMemo } from "react";
import { MessageBubble } from "@codeterm/chat-ui";
import { useChatSession } from "../../Terminal/useChatSession";
import { planAskReplies, type PlanAskReadyOwner } from "./planAskModel";

interface PlanAskExchangeProps {
  owner: PlanAskReadyOwner;
  busy: boolean;
  askId: string;
}

/** Read-only projection of the owner's existing transcript, narrowed to one question and its replies. */
export function PlanAskExchange({ owner, busy, askId }: PlanAskExchangeProps) {
  const { allMessages } = useChatSession({
    tabId: owner.tabId,
    agentSessionId: owner.agentSessionId,
    sessionBindingNonce: owner.sessionBindingNonce,
    allowUnboundAuthority: owner.chatBackend,
  });
  const { asked, replies } = useMemo(() => planAskReplies(allMessages, askId), [allMessages, askId]);
  const waiting = replies.length === 0;

  return (
    <div data-testid="plan-ask-exchange" className="plan-ask-exchange" aria-live="polite" aria-busy={waiting}>
      {replies.map((message) => (
        <div key={message.id} data-testid="plan-ask-reply" className="plan-ask-reply">
          <MessageBubble message={message} />
        </div>
      ))}
      {waiting && (
        <p data-testid="plan-ask-waiting" className="plan-ask-note">
          {busy ? `${owner.name} is busy. The reply appears here.` : asked ? "Waiting for the reply" : `Waiting for ${owner.name}`}
        </p>
      )}
    </div>
  );
}

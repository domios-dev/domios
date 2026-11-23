import type { ChatOrigin, NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "@codeterm/chat-ui";
import { chatMessageAccessibleName } from "../../../../../src/components/Terminal/chatMessageAccessibleName";
import { chatLayoutTokens } from "../../../../../src/components/Terminal/chatLayoutTokens";
import { ChatMessageRow, type MessageRowHandlers } from "../../../../../src/components/Terminal/ChatMessageRow";
import { easeOut, range } from "../../engine";

const noop = () => undefined;

const HANDLERS: MessageRowHandlers = {
  clipFor: () => null,
  partsFor: () => [],
  selectedFor: () => false,
  cursorFor: () => false,
  joinNextFor: () => false,
  select: noop,
  rowMouseDown: noop,
  toggle: noop,
  range: noop,
  endRange: noop,
  quote: noop,
  ask: noop,
};

export type TimedMessage = { message: NormalizedChatMessage; origin?: ChatOrigin; at: number };

/** TabSessionChat's message rows, each rising in at its own time. */
export function ChatRows({ t, rows, padding = "0 0.75rem 0.5rem" }: { t: number; rows: TimedMessage[]; padding?: string }) {
  return (
    <div className="ct-chat-shell w-full" style={{ ...chatLayoutTokens().variables, color: "var(--ct-fg)", padding }}>
      {rows.filter((row) => t >= row.at).map(({ message, origin, at }) => {
        const k = range(t, at, at + 0.35, easeOut);
        return (
          <div key={message.id} style={{ opacity: k, transform: `translateY(${(1 - k) * 14}px)` }}>
            <ChatMessageRow msg={message} ariaLabel={chatMessageAccessibleName(message.type, message.content)} selected={false} joinNext={false} selecting={false} cursor={false} handlers={HANDLERS}>
              <MessageBubble message={message} origin={origin} onOriginClick={noop} onOpenJobs={noop} onStopReminder={noop} />
            </ChatMessageRow>
          </div>
        );
      })}
    </div>
  );
}

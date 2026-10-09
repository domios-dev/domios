import type { ReactNode } from "react";
import { ChatRoundButton, type ChatRoundButtonState } from "./ChatRoundButton";
import "./chat-composer.css";

export function ChatBottomBar({ children }: { children: ReactNode }) {
  return <div className="ds-root ds-chat-bottom-bar">{children}</div>;
}

export interface ChatComposerMockProps {
  state?: ChatRoundButtonState;
  draft?: string;
}

export function ChatComposerMock({ state = "rest", draft = "" }: ChatComposerMockProps) {
  const recording = state === "recording" || state === "locked" || state === "cancel";
  const sending = !recording && draft.trim().length > 0;
  return <div className="ds-composer-dock">
    <div className="ds-composer-row" aria-label="Chat composer button placement">
      <ChatRoundButton kind="attach" state={state === "hover" ? "hover" : "rest"} />
      <div className="ds-composer-card" role="group" aria-label="Message composer">
        <div className="ds-composer-card__row">
          {recording
            ? <span className="ds-composer-card__recording"><span className="ds-composer-card__dot" /><span>0:08,42</span><span className="ds-composer-card__wave" aria-hidden>▁▃▅▂▆▃▇▂</span></span>
            : <span className={sending ? "ds-composer-card__text" : "ds-composer-card__text ds-composer-card__text--placeholder"}>{sending ? draft : "Message"}</span>}
          {sending && <span className="ds-composer-card__actions"><ChatRoundButton kind="schedule" /></span>}
        </div>
      </div>
      <span className="ds-composer-action-shell">
        <ChatRoundButton kind={sending ? "send" : "voice"} state={state === "hover" ? "rest" : state} />
      </span>
    </div>
  </div>;
}

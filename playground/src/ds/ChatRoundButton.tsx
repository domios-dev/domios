import { useCallback, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import "./chat-round-button.css";

export type ChatRoundButtonKind = "attach" | "voice" | "send" | "schedule" | "discard" | "play" | "stop";
export type ChatRoundButtonState = "rest" | "hover" | "active" | "recording" | "locked" | "cancel";

export interface ChatRoundButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  kind?: ChatRoundButtonKind;
  state?: ChatRoundButtonState;
  label?: string;
  children?: ReactNode;
}

const labels: Record<ChatRoundButtonKind, string> = {
  attach: "Attach file",
  voice: "Hold to record voice note",
  send: "Send",
  schedule: "Schedule send",
  discard: "Delete recording",
  play: "Play recording",
  stop: "Stop and review voice note",
};

function Icon({ kind, state }: Pick<ChatRoundButtonProps, "kind" | "state">) {
  if (state === "cancel") return <svg viewBox="0 0 24 24" aria-hidden><path d="m6 6 12 12M18 6 6 18" /></svg>;
  if (kind === "attach") return <svg viewBox="0 0 24 24" aria-hidden><path d="m8.4 12.7 6.2-6.2a3.1 3.1 0 1 1 4.4 4.4l-8.4 8.4a5 5 0 0 1-7.1-7.1l8-8" /></svg>;
  if (kind === "send") return <svg viewBox="0 0 24 24" aria-hidden><path d="m4.5 5.5 16 6.5-16 6.5 4-6.5-4-6.5Zm4 6.5h6" /></svg>;
  if (kind === "schedule") return <svg viewBox="0 0 24 24" aria-hidden><circle cx="12" cy="12" r="7" /><path d="M12 8v4l2.8 1.7" /></svg>;
  if (kind === "discard") return <svg viewBox="0 0 24 24" aria-hidden><path d="M5 7h14M10 11v6m4-6v6M9 7l.7-2h4.6l.7 2M7.5 7l.6 12h7.8l.6-12" /></svg>;
  if (kind === "play") return <svg viewBox="0 0 24 24" aria-hidden><path d="m9 7 8 5-8 5Z" fill="currentColor" /></svg>;
  if (kind === "stop" || state === "locked") return <svg viewBox="0 0 24 24" aria-hidden><rect x="7" y="7" width="10" height="10" fill="currentColor" /></svg>;
  return <svg viewBox="0 0 24 24" aria-hidden><rect x="9" y="5" width="6" height="10" rx="3" /><path d="M6.5 12a5.5 5.5 0 0 0 11 0M12 17.5V20M9 20h6" /></svg>;
}

export function ChatRoundButton({ kind = "voice", state = "rest", label, className = "", children, onPointerUp, onAnimationEnd, ...props }: ChatRoundButtonProps) {
  const [releasing, setReleasing] = useState(false);
  const armRelease = useCallback((event: React.PointerEvent<HTMLButtonElement>) => {
    setReleasing(true);
    onPointerUp?.(event);
  }, [onPointerUp]);
  const disarmRelease = useCallback((event: React.AnimationEvent<HTMLButtonElement>) => {
    setReleasing(false);
    onAnimationEnd?.(event);
  }, [onAnimationEnd]);
  const isRecording = state === "recording" || state === "locked" || state === "cancel";
  return <button type="button" className={`ds-chat-round-button ds-chat-round-button--${kind} ds-chat-round-button--${state}${releasing ? " is-releasing" : ""} ${className}`} aria-label={label ?? labels[kind]} aria-pressed={isRecording || undefined} onPointerUp={armRelease} onAnimationEnd={disarmRelease} {...props}>
    {children ?? <Icon kind={kind} state={state} />}
  </button>;
}

export function ChatComposerMock({ state = "rest" }: { state?: ChatRoundButtonState }) {
  const recording = state === "recording" || state === "locked" || state === "cancel";
  return <div className="ds-chat-composer" aria-label="Chat composer button placement">
    {!recording && <ChatRoundButton kind="attach" state={state === "hover" ? "hover" : "rest"} />}
    <div className={`ds-chat-composer__shell${recording ? " is-recording" : ""}`}>
      {recording ? <><span className="ds-chat-composer__dot" /><span>0:08,42</span><span className="ds-chat-composer__wave" aria-hidden>▁▃▅▂▆▃▇▂</span></> : <><span className="ds-chat-composer__placeholder">Message CodeTerm…</span><ChatRoundButton kind="schedule" /></>}
    </div>
    {!recording && <ChatRoundButton kind={state === "active" ? "send" : "voice"} state={state} disabled={state === "active" && false} />}
    {recording && <ChatRoundButton kind="voice" state={state} />}
  </div>;
}

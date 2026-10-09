import { useCallback, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Clock3, Mic, Paperclip, Play, Square, Trash2, X } from "lucide-react";
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

const ICON = { size: 17, strokeWidth: 2.2 } as const;

export function SendIcon({ size = 14 }: { size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden><path d="M4.5 12H19M13 5.5 19.5 12 13 18.5" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" /></svg>;
}

function BloomGlyphs() {
  return <>
    <span className="ds-send-bloom" aria-hidden />
    <span className="ds-send-glyph ds-send-glyph--mic" aria-hidden><Mic {...ICON} /></span>
    <span className="ds-send-glyph ds-send-glyph--send" aria-hidden><SendIcon size={ICON.size} /></span>
  </>;
}

function Icon({ kind, state }: Required<Pick<ChatRoundButtonProps, "kind" | "state">>) {
  if (state === "cancel") return <X size={30} strokeWidth={2.6} />;
  if (state === "locked") return <Square size={18} fill="currentColor" />;
  if (state === "recording") return <Mic size={30} strokeWidth={2.4} />;
  if (kind === "attach") return <Paperclip {...ICON} />;
  if (kind === "schedule") return <Clock3 size={14} strokeWidth={2.2} />;
  if (kind === "discard") return <Trash2 size={18} strokeWidth={2.2} />;
  if (kind === "play") return <Play size={16} fill="currentColor" />;
  if (kind === "stop") return <Square size={18} fill="currentColor" />;
  return <BloomGlyphs />;
}

export function ChatRoundButton({ kind = "voice", state = "rest", label, className = "", children, onPointerUp, onAnimationEnd, ...props }: ChatRoundButtonProps) {
  const [releasing, setReleasing] = useState(false);
  const armRelease = useCallback((event: React.PointerEvent<HTMLButtonElement>) => {
    setReleasing(true);
    onPointerUp?.(event);
  }, [onPointerUp]);
  const disarmRelease = useCallback((event: React.AnimationEvent<HTMLButtonElement>) => {
    if (event.target === event.currentTarget) setReleasing(false);
    onAnimationEnd?.(event);
  }, [onAnimationEnd]);
  const isRecording = state === "recording" || state === "locked" || state === "cancel";
  const isAction = kind === "voice" || kind === "send";
  return <button type="button" className={`ds-chat-round-button ds-chat-round-button--${kind} ds-chat-round-button--${state}${releasing ? " is-releasing" : ""} ${className}`} data-send={isAction && !isRecording ? String(kind === "send") : undefined} aria-label={label ?? labels[kind]} aria-pressed={isRecording || undefined} onPointerUp={armRelease} onAnimationEnd={disarmRelease} {...props}>
    {children ?? <Icon kind={kind} state={state} />}
  </button>;
}

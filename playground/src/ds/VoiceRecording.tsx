import type { CSSProperties, ReactNode } from "react";
import "./voice-recording.css";

export type VoiceRecordingState = "idle" | "recording" | "sliding" | "locked" | "preview" | "cancel";

export interface VoiceRecordingProps {
  state?: VoiceRecordingState;
  elapsed?: string;
  peaks?: number[];
  progress?: number;
  playing?: boolean;
  disabled?: boolean;
  onStart?: () => void;
  onLock?: () => void;
  onCancel?: () => void;
  onStop?: () => void;
  onDiscard?: () => void;
  onTogglePlayback?: () => void;
  onSend?: () => void;
}

const defaultPeaks = [0.2, 0.46, 0.3, 0.68, 0.38, 0.82, 0.5, 0.27, 0.62, 0.4, 0.74, 0.32, 0.54, 0.25, 0.66, 0.4];

function Icon({ name }: { name: "mic" | "lock" | "stop" | "play" | "pause" | "send" | "trash" | "close" | "chevron" }) {
  const paths = {
    mic: <><rect x="9" y="4" width="6" height="10" rx="3" /><path d="M6.5 11a5.5 5.5 0 0 0 11 0M12 16.5V20M9 20h6" /></>,
    lock: <><rect x="6.5" y="10" width="11" height="9" rx="1.5" /><path d="M9 10V7.5a3 3 0 0 1 6 0V10" /></>,
    stop: <rect x="7" y="7" width="10" height="10" rx="1" fill="currentColor" stroke="none" />,
    play: <path d="m9 7 8 5-8 5Z" fill="currentColor" stroke="none" />,
    pause: <><rect x="8" y="7" width="3" height="10" fill="currentColor" stroke="none" /><rect x="13" y="7" width="3" height="10" fill="currentColor" stroke="none" /></>,
    send: <path d="m4.5 5.5 16 6.5-16 6.5 4-6.5-4-6.5Zm4 6.5h6" />,
    trash: <><path d="M5 7h14M10 11v6m4-6v6M9 7l.7-2h4.6l.7 2M7.5 7l.6 12h7.8l.6-12" /></>,
    close: <path d="m7 7 10 10M17 7 7 17" />,
    chevron: <path d="m7 14 5-5 5 5" />,
  };
  return <svg viewBox="0 0 24 24" aria-hidden>{paths[name]}</svg>;
}

function Waveform({ peaks, progress = 0, active = false }: { peaks: number[]; progress?: number; active?: boolean }) {
  return <div className="ds-voice-waveform" aria-hidden>{peaks.map((peak, index) => {
    const played = !active && index / peaks.length <= progress;
    return <span key={index} className={played ? "is-played" : ""} style={{ "--voice-bar": `${Math.max(14, peak * 100)}%` } as CSSProperties} />;
  })}</div>;
}

function RoundButton({ label, className = "", onClick, children, disabled }: { label: string; className?: string; onClick?: () => void; children: ReactNode; disabled?: boolean }) {
  return <button type="button" className={`ds-voice-round ${className}`} aria-label={label} disabled={disabled} onClick={onClick}>{children}</button>;
}

export function VoiceRecording({
  state = "idle", elapsed = "0:08,42", peaks = defaultPeaks, progress = 0.38, playing = false, disabled = false,
  onStart, onLock, onCancel, onStop, onDiscard, onTogglePlayback, onSend,
}: VoiceRecordingProps) {
  const recording = state === "recording" || state === "sliding" || state === "locked" || state === "cancel";
  const cancelArmed = state === "sliding" || state === "cancel";
  if (state === "preview") return <div className="ds-voice-recording ds-voice-recording--preview" aria-label="Voice note preview">
    <RoundButton label="Delete recording" className="is-delete" onClick={onDiscard} disabled={disabled}><Icon name="trash" /></RoundButton>
    <RoundButton label={playing ? "Pause recording" : "Play recording"} className="is-primary" onClick={onTogglePlayback} disabled={disabled}><Icon name={playing ? "pause" : "play"} /></RoundButton>
    <Waveform peaks={peaks} progress={progress} />
    <output className="ds-voice-time">0:12</output>
    <RoundButton label="Send voice note" className="is-primary" onClick={onSend} disabled={disabled}><Icon name="send" /></RoundButton>
  </div>;

  return <div className={`ds-voice-recording ds-voice-recording--${state}`} aria-label={recording ? "Voice recording in progress" : "Voice recording ready"}>
    <div className="ds-voice-composer">
      {recording ? <><span className="ds-voice-dot" /><output className="ds-voice-time">{elapsed}</output><Waveform peaks={peaks} active /></> : <span className="ds-voice-placeholder">Message</span>}
      {recording && state !== "locked" && <button type="button" className={`ds-voice-cancel-hint${cancelArmed ? " is-armed" : ""}`} onClick={onCancel}><Icon name="chevron" /><span>{cancelArmed ? "Release to cancel" : "Slide to cancel"}</span></button>}
      {state === "locked" && <button type="button" className="ds-voice-text-button" onClick={onCancel}>Cancel</button>}
    </div>
    <div className="ds-voice-action-wrap">
      {(state === "recording" || state === "sliding") && <button type="button" className="ds-voice-lock-pill" aria-label="Slide up to lock recording" onClick={onLock}><Icon name="lock" /><Icon name="chevron" /></button>}
      <RoundButton label={state === "locked" ? "Stop and review voice note" : "Hold to record voice note"} className={recording ? `is-recording${cancelArmed ? " is-cancel" : ""}` : ""} onClick={state === "locked" ? onStop : onStart} disabled={disabled}><Icon name={cancelArmed ? "close" : state === "locked" ? "stop" : "mic"} /></RoundButton>
    </div>
  </div>;
}

export { Waveform as VoiceWaveform };

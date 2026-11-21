import { useRef } from "react";
import { Mic, Square, X } from "lucide-react";
import { Button } from "../../shared/Button";
import { ActivityDots } from "../../shared/ActivityDots";
import { isCancelGesture, isDragUpGesture } from "./voiceFsm";
import type { LiveDictation } from "./useLiveDictation";

const TAP_MS = 220;

export function DictationControls({ dictation, disabled }: { dictation: LiveDictation; disabled: boolean }) {
  const pointer = useRef<{ id: number; x: number; y: number; at: number; locked: boolean } | null>(null);
  const active = dictation.phase !== "idle";
  const toggle = () => { if (dictation.active()) void dictation.stop(); else void dictation.start(); };
  return <div className="ct-dictation-controls flex shrink-0 items-center gap-1">
    {active && <Button type="button" variant="ghost" aria-label="Cancel dictation" title="Cancel dictation" onClick={dictation.cancel}>
      <X size={15} />
    </Button>}
    <Button
      type="button" variant="ghost"
      data-testid="composer-voice-button"
      aria-label={active ? "Stop dictation" : "Start dictation"}
      title={active ? "Stop dictation" : "Dictation · tap, hold, or slide up to lock"}
      className="ct-dictation-mic"
      disabled={disabled || dictation.phase === "finishing"}
      aria-pressed={active}
      onPointerDown={event => {
        if (event.isPrimary === false || event.button !== 0) return;
        event.preventDefault();
        if (dictation.active()) { void dictation.stop(); return; }
        pointer.current = { id: event.pointerId, x: event.clientX, y: event.clientY, at: event.timeStamp, locked: false };
        event.currentTarget.setPointerCapture?.(event.pointerId);
        void dictation.start();
      }}
      onPointerMove={event => {
        const gesture = pointer.current;
        if (gesture?.id === event.pointerId && isDragUpGesture(gesture.x, gesture.y, event.clientX, event.clientY)) gesture.locked = true;
      }}
      onPointerUp={event => {
        const gesture = pointer.current;
        if (gesture?.id !== event.pointerId) return;
        pointer.current = null;
        if (gesture.locked || isDragUpGesture(gesture.x, gesture.y, event.clientX, event.clientY)) return;
        if (isCancelGesture(gesture.x, event.clientX, false)) dictation.cancel();
        else if (event.timeStamp - gesture.at > TAP_MS) void dictation.stop();
      }}
      onPointerCancel={event => {
        const gesture = pointer.current;
        if (gesture?.id !== event.pointerId) return;
        pointer.current = null;
        if (!gesture.locked) dictation.cancel();
      }}
      onLostPointerCapture={event => {
        const gesture = pointer.current;
        if (gesture?.id !== event.pointerId) return;
        pointer.current = null;
        if (!gesture.locked) dictation.cancel();
      }}
      onClick={event => { if (!event.detail) toggle(); }}
    >
      {active ? <Square size={15} /> : <Mic size={17} />}
    </Button>
  </div>;
}

export function DictationStatus({ dictation }: { dictation: LiveDictation }) {
  if (dictation.error) return <span role="alert" className="ct-dictation-error">{dictation.error}</span>;
  if (dictation.phase === "idle") return null;
  const label = dictation.phase === "starting" ? "Starting dictation" : dictation.phase === "finishing" ? "Finishing dictation" : "Listening";
  return <span role="status" className="ct-dictation-status">
    {dictation.phase === "listening" ? <span className="ct-dictation-listening" aria-hidden="true" /> : <ActivityDots size="small" tone="muted" />}
    {label}
  </span>;
}

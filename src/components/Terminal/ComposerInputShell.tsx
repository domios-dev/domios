import { Clock3 } from "lucide-react";
import type { ReactNode } from "react";
import { VoicePreview } from "./voice/VoicePreview";
import { VoiceRecordingBar } from "./voice/VoiceRecordingBar";
import type { ComposerVoiceHandle } from "./voice/useComposerVoice";

const btnBase =
  "ct-liquid-interactive flex items-center justify-center shrink-0";

export function ComposerInputShell({
  voiceUi,
  onSchedule,
  children,
}: {
  voiceUi: ComposerVoiceHandle;
  onSchedule?: () => void;
  children: ReactNode;
}) {
  const {
    voice,
    shellRef,
    cancelHintRef,
    voiceBusy,
    cancelArmed,
    isLocked,
    isPreview,
    composerHidden,
    mainIsSend,
    cancelVoice,
    discardPreviewClip,
    sendPreview,
  } = voiceUi;

  return (
    <div
      ref={shellRef}
      data-testid="composer-input-shell"
      className="ct-composer-input-shell relative flex min-h-10 flex-1 items-center rounded-[1.35rem] border px-3.5 py-2 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] focus-within:ring-2 focus-within:ring-[var(--ct-accent)]"
      style={{
        background: "var(--ct-bg-elev, var(--ct-surface, #161b22))",
        borderColor: "var(--ct-border, #30363d)",
      }}
    >
      {composerHidden && (
        <div className="absolute inset-1 z-10 flex items-center px-1.5">
          {isPreview && voice.preview ? (
            <VoicePreview
              data={voice.preview}
              busy={voiceBusy}
              onDiscard={discardPreviewClip}
              onSend={() => void sendPreview()}
            />
          ) : (
            <VoiceRecordingBar
              mode={isLocked ? "locked" : "hold"}
              armed={cancelArmed}
              peaks={voice.livePeaks}
              cancelHintRef={cancelHintRef}
              onCancel={() => void cancelVoice()}
            />
          )}
        </div>
      )}
      <div
        className={
          composerHidden
            ? "relative flex min-w-0 flex-1 items-center opacity-0 pointer-events-none"
            : "relative flex min-w-0 flex-1 items-center"
        }
      >
        {children}
      </div>
      {!composerHidden && mainIsSend && onSchedule && (
        <ComposerScheduleButton onSchedule={onSchedule} />
      )}
    </div>
  );
}

function ComposerScheduleButton({ onSchedule }: { onSchedule: () => void }) {
  return (
    <button
      type="button"
      data-testid="composer-schedule-button"
      title="Schedule send"
      onMouseDown={(e) => e.preventDefault()}
      onClick={(e) => {
        e.preventDefault();
        onSchedule();
      }}
      className={`${btnBase} ct-composer-round-button ml-1 h-5 w-5 shrink-0 self-center rounded-full text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:brightness-125`}
      style={{
        background: "var(--ct-surface, #161b22)",
        border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 18%, transparent)",
      }}
    >
      <Clock3 size={13} strokeWidth={2.2} />
    </button>
  );
}

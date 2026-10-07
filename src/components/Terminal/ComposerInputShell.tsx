import { ComposerCard } from "./ComposerCard";
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
  header,
  footer,
  actions,
}: {
  voiceUi: ComposerVoiceHandle;
  onSchedule?: () => void;
  children: ReactNode;
  header?: ReactNode;
  footer?: ReactNode;
  actions?: ReactNode;
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
    <ComposerCard ref={shellRef}
      header={!composerHidden ? header : undefined}
      footer={<>
        <div className={composerHidden ? "invisible flex flex-wrap items-center gap-1" : "flex flex-wrap items-center gap-1"} inert={composerHidden || undefined}>{footer}</div>
        {!composerHidden && <div className="flex items-center gap-2">
          {actions}
          {mainIsSend && onSchedule && <ComposerScheduleButton onSchedule={onSchedule} />}
        </div>}
      </>}
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
    </ComposerCard>
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

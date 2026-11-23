import { ComposerCard } from "./ComposerCard";
import type { ReactNode } from "react";
import { VoicePreview } from "./voice/VoicePreview";
import { VoiceRecordingBar } from "./voice/VoiceRecordingBar";
import type { ComposerVoiceHandle } from "./voice/useComposerVoice";

export function ComposerInputShell({
  voiceUi,
  dictationPhase,
  children,
  header,
  footer,
  actions,
  overlay,
  preview,
}: {
  voiceUi: ComposerVoiceHandle;
  dictationPhase?: "idle" | "starting" | "listening" | "finishing";
  children: ReactNode;
  header?: ReactNode;
  footer?: ReactNode;
  actions?: ReactNode;
  overlay?: ReactNode;
  /** The website film's read-only composer: the editor is inert and an optional overlay shows the scripted text. */
  preview?: { inputOverlay?: ReactNode } | null;
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
    cancelVoice,
    discardPreviewClip,
    sendPreview,
  } = voiceUi;

  const hideEditor = composerHidden || Boolean(overlay);

  return (
    <ComposerCard ref={shellRef}
      dictating={!hideEditor && dictationPhase && dictationPhase !== "idle" ? dictationPhase : undefined}
      header={!composerHidden ? header : undefined}
      footer={!hideEditor && footer ? <div className="flex flex-wrap items-center gap-1">{footer}</div> : undefined}
      actions={!hideEditor && actions && <div className="ct-composer-inner-actions flex shrink-0 items-center gap-1">{actions}</div>}
    >
      {preview?.inputOverlay && <div className="absolute inset-1 z-10 flex items-center px-1.5">{preview.inputOverlay}</div>}
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
      {!composerHidden && overlay && (
        <div className="absolute inset-1 z-10 flex items-center px-2">
          {overlay}
        </div>
      )}
      <div inert={!!preview} aria-hidden={preview ? true : undefined}
        className={
          hideEditor || !!preview?.inputOverlay
            ? "relative flex min-w-0 flex-1 items-center opacity-0 pointer-events-none"
            : "relative flex min-w-0 flex-1 items-center"
        }
      >
        {children}
      </div>
    </ComposerCard>
  );
}

import { ComposerCard } from "./ComposerCard";
import type { ReactNode } from "react";
import { VoicePreview } from "./voice/VoicePreview";
import { VoiceRecordingBar } from "./voice/VoiceRecordingBar";
import type { ComposerVoiceHandle } from "./voice/useComposerVoice";

export function ComposerInputShell({
  voiceUi,
  children,
  header,
  footer,
  actions,
}: {
  voiceUi: ComposerVoiceHandle;
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
    cancelVoice,
    discardPreviewClip,
    sendPreview,
  } = voiceUi;

  return (
    <ComposerCard ref={shellRef}
      header={!composerHidden ? header : undefined}
      footer={!composerHidden && footer ? <div className="flex flex-wrap items-center gap-1">{footer}</div> : undefined}
      actions={!composerHidden && actions && <div className="ct-composer-inner-actions flex shrink-0 items-center gap-1">{actions}</div>}
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

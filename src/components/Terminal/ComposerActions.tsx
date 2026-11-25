import { useRef, type ReactNode } from "react";
import { ComposerActionButton } from "./ComposerActionButton";
import { ComposerAttachButton } from "./ComposerAttachButton";
import { ComposerInputShell } from "./ComposerInputShell";
import { ComposerScheduleButton } from "./ComposerScheduleButton";
import { useAttachmentUploads } from "./useAttachmentUploads";
import { useComposerVoice } from "./voice/useComposerVoice";
import { ComposerIssuesProvider, useHasComposerIssuesProvider } from "./ComposerIssuesContext";
import { DictationPresence } from "./voice/DictationStatus";
import type { LiveDictation } from "./voice/useLiveDictation";
import { remoteDictationUnavailable } from "../../lib/transport/dictation";

export { storeBrowserFiles, useWebFileDrop } from "./composerFiles";

interface Props {
  expanded?: boolean;
  tabId?: string;
  onAttach: (paths: string[]) => void;
  onSend: () => void;
  onSchedule?: () => void;
  onSendWithAttachments: (paths: string[]) => void;
  onError: (msg: string) => void;
  sendDisabled: boolean;
  sendUnavailableReason?: string | null;
  hasText: boolean;
  children: ReactNode;
  cardHeader?: ReactNode;
  cardFooter?: ReactNode;
  inputOverlay?: ReactNode;
  hideActionButton?: boolean;
  canAttach?: boolean;
  uploadFiles?: (files: File[]) => Promise<void>;
  dictation?: LiveDictation;
  onOpenLiveTranscript?: () => void;
  liveTranscriptOnTap?: boolean;
  spaceArming?: boolean;
  spacePop?: boolean;
}

export function ComposerActions(props: Props) {
  const provided = useHasComposerIssuesProvider();
  return provided ? <ComposerActionsBody {...props} /> : <ComposerIssuesProvider>
    <ComposerActionsBody {...props} />
  </ComposerIssuesProvider>;
}

function ComposerActionsBody({
  expanded = false,
  tabId,
  onAttach,
  onSend,
  onSchedule,
  onSendWithAttachments,
  onError,
  sendDisabled,
  sendUnavailableReason,
  hasText,
  children,
  cardHeader,
  cardFooter,
  inputOverlay,
  hideActionButton = false,
  canAttach = true,
  uploadFiles,
  dictation,
  onOpenLiveTranscript,
  liveTranscriptOnTap,
  spaceArming = false,
  spacePop = false,
}: Props) {
  const fileRef = useRef<HTMLInputElement>(null);
  const uploads = useAttachmentUploads(onAttach);
  const voiceUploads = useAttachmentUploads(onSendWithAttachments);
  const voiceUi = useComposerVoice({
    tabId,
    hasText,
    onSend,
    onAttach,
    onSendWithAttachments,
    onError,
    persistBlob: voiceUploads.store,
  });

  const attachButton = canAttach && !voiceUi.isPreview
    && <ComposerAttachButton uploading={uploads.busy} onClick={() => fileRef.current?.click()} />;
  const dictationActive = Boolean(dictation && dictation.phase !== "idle");
  const scheduleVisible = voiceUi.mainIsSend && !dictationActive && Boolean(onSchedule);

  return (
    <>
      {canAttach && <input
        ref={fileRef}
        type="file"
        multiple
        className="hidden"
        data-testid="composer-attach-input"
        onChange={(e) => {
          const files = e.target.files;
          if (!files || files.length === 0) return;
          void (uploadFiles ?? uploads.addFiles)(Array.from(files)).finally(() => {
            if (fileRef.current) fileRef.current.value = "";
          });
        }}
      />}
      {uploads.status}
      {voiceUploads.status}
      <div className={expanded ? "ct-expanded-composer__actions relative grid w-full items-end gap-2" : "relative flex w-full items-end gap-1.5"}>
        {attachButton}
        <ComposerInputShell voiceUi={voiceUi} dictationPhase={dictation?.phase} header={cardHeader} footer={cardFooter} overlay={inputOverlay}
          actions={!expanded
            ? (dictationActive && dictation
              ? <DictationPresence dictation={dictation} />
              : (scheduleVisible && onSchedule && <ComposerScheduleButton onSchedule={onSchedule} />))
            : undefined}>
          {children}
        </ComposerInputShell>
        {expanded && dictationActive && dictation && (
          <div className="ct-expanded-composer__dictation">
            <DictationPresence dictation={dictation} />
          </div>
        )}
        {expanded && scheduleVisible && onSchedule && <div className="ct-expanded-composer__schedule"><ComposerScheduleButton onSchedule={onSchedule} /></div>}
        {!voiceUi.isPreview && !hideActionButton && (
          <ComposerActionButton
            voiceUi={voiceUi}
            dictation={dictation}
            dictationUnavailableReason={dictation ? remoteDictationUnavailable(tabId) : undefined}
            onOpenLiveTranscript={onOpenLiveTranscript}
            liveTranscriptOnTap={liveTranscriptOnTap}
            spaceArming={spaceArming}
            spacePop={spacePop}
            sendDisabled={sendDisabled}
            sendUnavailableReason={sendUnavailableReason}
            onSend={onSend}
            onSchedule={onSchedule}
          />
        )}
      </div>
    </>
  );
}

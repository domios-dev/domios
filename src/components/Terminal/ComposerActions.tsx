import { createPortal } from "react-dom";
import { usePhoneViewport } from "../../hooks/useSidebarViewport";
import { useRef, type ReactNode } from "react";
import { ComposerActionButton } from "./ComposerActionButton";
import { ComposerAttachButton } from "./ComposerAttachButton";
import { ComposerInputShell } from "./ComposerInputShell";
import { ComposerScheduleButton } from "./ComposerScheduleButton";
import { useAttachmentUploads } from "./useAttachmentUploads";
import { useComposerVoice } from "./voice/useComposerVoice";
import { ComposerIssuesProvider, useHasComposerIssuesProvider } from "./ComposerIssuesContext";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";

export { storeBrowserFiles, useWebFileDrop } from "./composerFiles";

interface Props {
  tabId?: string;
  attachSlot?: HTMLElement | null;
  onAttach: (paths: string[]) => void;
  onSend: () => void;
  onSchedule?: () => void;
  onSendWithAttachments: (paths: string[]) => void;
  onError: (msg: string) => void;
  sendDisabled: boolean;
  hasText: boolean;
  children: ReactNode;
  cardHeader?: ReactNode;
  cardFooter?: ReactNode;
  canAttach?: boolean;
  uploadFiles?: (files: File[]) => Promise<void>;
}

export function ComposerActions(props: Props) {
  const provided = useHasComposerIssuesProvider();
  return provided ? <ComposerActionsBody {...props} /> : <ComposerIssuesProvider>
    <AgentRuntimeBanner state={null} onRetry={() => {}} />
    <ComposerActionsBody {...props} />
  </ComposerIssuesProvider>;
}

function ComposerActionsBody({
  tabId,
  attachSlot,
  onAttach,
  onSend,
  onSchedule,
  onSendWithAttachments,
  onError,
  sendDisabled,
  hasText,
  children,
  cardHeader,
  cardFooter,
  canAttach = true,
  uploadFiles,
}: Props) {
  const phone = usePhoneViewport();
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

  return (
    <>
      {phone && attachSlot && createPortal(attachButton, attachSlot)}
      {phone && !attachSlot && <div className="mb-1">{attachButton}</div>}
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
      <div className="relative flex w-full items-end gap-1.5">
        {!phone && attachButton}
        <ComposerInputShell voiceUi={voiceUi} header={cardHeader} footer={cardFooter}
          actions={voiceUi.mainIsSend && onSchedule && <ComposerScheduleButton onSchedule={onSchedule} />}>
          {children}
        </ComposerInputShell>
        {!voiceUi.isPreview && (
          <ComposerActionButton
            voiceUi={voiceUi}
            sendDisabled={sendDisabled}
            onSend={onSend}
            onSchedule={onSchedule}
          />
        )}
      </div>
    </>
  );
}

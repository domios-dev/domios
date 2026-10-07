import { useRef, useState, type ReactNode } from "react";
import { ComposerActionButton } from "./ComposerActionButton";
import { ComposerAttachButton } from "./ComposerAttachButton";
import { ComposerInputShell } from "./ComposerInputShell";
import { storeBrowserFiles } from "./composerFiles";
import { useComposerVoice } from "./voice/useComposerVoice";

export { storeBrowserFiles, useWebFileDrop } from "./composerFiles";

interface Props {
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
}

export function ComposerActions({
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
}: Props) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [uploading, setUploading] = useState(false);
  const voiceUi = useComposerVoice({
    hasText,
    onSend,
    onAttach,
    onSendWithAttachments,
    onError,
  });

  return (
    <>
      <input
        ref={fileRef}
        type="file"
        multiple
        className="hidden"
        data-testid="composer-attach-input"
        onChange={(e) => {
          const files = e.target.files;
          if (!files || files.length === 0) return;
          setUploading(true);
          void storeBrowserFiles(Array.from(files), onAttach, onError).finally(() => {
            setUploading(false);
            if (fileRef.current) fileRef.current.value = "";
          });
        }}
      />
      <div className="relative flex w-full items-end gap-1.5">
        <ComposerInputShell voiceUi={voiceUi} onSchedule={onSchedule} header={cardHeader} footer={cardFooter}
          actions={!voiceUi.isPreview && <ComposerAttachButton uploading={uploading} onClick={() => fileRef.current?.click()} />}>
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

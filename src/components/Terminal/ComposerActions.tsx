/**
 * Composer affordances for every platform: attach-file, voice note,
 * explicit send (touch screens have no Enter habit).
 *
 * Browser files can't be referenced by path — storeBlob uploads them to
 * the daemon (POST /api/upload) in web mode or writes a temp file via the
 * Tauri command in desktop mode; either way the attached path is readable
 * by the agent CLI. Voice notes ride the same pipe; if the receiving tab
 * has no transcriber it will say so — the capability is the composer's
 * job, transcription is the tab's.
 *
 * Voice capture: hold the mic to record, slide left to cancel, slide up
 * to lock hands-free, then stop into a listen-before-send preview.
 * State machine + waveform live in ./voice/.
 */
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
        {!voiceUi.isPreview && (
          <ComposerAttachButton uploading={uploading} onClick={() => fileRef.current?.click()} />
        )}
        <ComposerInputShell voiceUi={voiceUi} onSchedule={onSchedule}>
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

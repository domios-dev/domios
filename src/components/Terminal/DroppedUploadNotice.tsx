import { useRef, useState } from "react";
import { formatAttachmentMarker, attachmentKindForUpload } from "@codeterm/chat-core";
import { canRetryUpload, classifyUploadFailure, type AttachmentUpload } from "./attachmentUploadModel";
import { addChatSend, setDroppedUploads, failChatSendRequest, updateChatSendContent } from "./chatSendStore";
import { sendTrackedChatMessage } from "./chatSendStatus";
import { validatedStore } from "./composerUploadModel";
import { storeBlob } from "./voice/audio";
import { uid } from "../../lib/uid";
import { composeAttachmentDraft } from "./meshAttachmentHandoff";
import { composerAttachmentHandoff } from "./composerAttachmentHandoff";

const captures = new Map<string, AttachmentUpload[]>();
const key = (tabId: string, id: string) => JSON.stringify([tabId, id]);
export function recordDroppedUploads(tabId: string, id: string, uploads: AttachmentUpload[]) {
  if (!uploads.length) return;
  captures.set(key(tabId, id), uploads.filter(canRetryUpload));
  while (captures.size > 64) captures.delete(captures.keys().next().value!);
  setDroppedUploads(tabId, id, uploads.map(upload => upload.failure ? `${upload.name}: ${upload.failure.message}` : upload.name));
}

export function DroppedUploadNotice({ tabId, id, names }: { tabId: string; id: string; names?: string[] }) {
  const [busy, setBusy] = useState(false);
  const running = useRef(false);
  if (!names?.length) return null;
  async function retry() {
    if (running.current) return;
    const uploads = captures.get(key(tabId, id));
    if (!uploads?.length) return;
    running.current = true;
    setBusy(true);
    const remaining: AttachmentUpload[] = [];
    for (const upload of uploads) {
      let path: string;
      try { path = await validatedStore(storeBlob, upload.blob, upload.name); }
      catch (error) {
        const failed: AttachmentUpload = { ...upload, state: "failed", failure: classifyUploadFailure(error) };
        remaining.push(failed); continue;
      }
      const sendId = uid();
      const content = formatAttachmentMarker(path, attachmentKindForUpload({ name: upload.name, mime: upload.blob.type }));
      addChatSend(tabId, { id: sendId, content, sentAtMs: Date.now() });
      try {
        const prepared = await composeAttachmentDraft(tabId, "", [path], [], composerAttachmentHandoff);
        updateChatSendContent(tabId, sendId, prepared.composed);
        await sendTrackedChatMessage(tabId, sendId, prepared.composed).catch(() => {});
      } catch { failChatSendRequest(tabId, sendId); }
    }
    if (remaining.some(canRetryUpload)) captures.set(key(tabId, id), remaining.filter(canRetryUpload));
    else captures.delete(key(tabId, id));
    setDroppedUploads(tabId, id, remaining.map(upload => upload.failure ? `${upload.name}: ${upload.failure.message}` : upload.name));
    running.current = false;
    setBusy(false);
  }
  return <span role="note" className="inline-flex min-w-0 flex-wrap items-center gap-1 text-xs text-[var(--ct-muted-fg)]">
    <span>{names.join(", ")} was not attached.</span>
    {captures.get(key(tabId, id))?.length ? <button type="button" disabled={busy} onClick={() => void retry()} aria-label="Retry omitted attachments" className="shrink-0 whitespace-nowrap">Retry</button> : null}
  </span>;
}

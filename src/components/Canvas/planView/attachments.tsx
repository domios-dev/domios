import { useMemo, useRef, useState } from "react";
import { ImageFullscreen } from "@codeterm/chat-ui";
import type { ApiConfig } from "../../../lib/types";
import { baseUrl } from "../../MemoryPanel/memoryApi";
import type { PlanAttachment } from "../types";

export function planAttachmentUrl(api: ApiConfig | undefined, planId: string | undefined, attachment: PlanAttachment): string {
  if (!planId) return "";
  const root = api ? baseUrl(api) : "";
  return `${root}/api/plans/${encodeURIComponent(planId)}/attachments?path=${encodeURIComponent(attachment.path)}`;
}

export function useImageAttachments(api: ApiConfig | undefined, planId: string | undefined, attachments: readonly PlanAttachment[] | undefined) {
  return useMemo(
    () => (attachments ?? [])
      .filter((attachment) => attachment.media_type.startsWith("image/"))
      .map((attachment) => ({
        attachment,
        url: planAttachmentUrl(api, planId, attachment),
      }))
      .filter((item) => item.url),
    [api, attachments, planId],
  );
}

export function AttachmentGallery({ api, planId, attachments }: { api?: ApiConfig; planId?: string; attachments?: readonly PlanAttachment[] }) {
  const galleryRef = useRef<HTMLDivElement>(null);
  const images = useImageAttachments(api, planId, attachments);
  const [open, setOpen] = useState<{ name: string; url: string } | null>(null);
  if (images.length === 0) return null;
  return (
    <>
      <div ref={galleryRef} style={{ display: "flex", gap: 8, flexWrap: "wrap" }} data-testid="plan-attachment-gallery">
        {images.map(({ attachment, url }) => (
          <button
            key={attachment.id}
            type="button"
            title={attachment.name}
            aria-label={`Open image ${attachment.name}`}
            onClick={(event) => { event.currentTarget.focus(); setOpen({ name: attachment.name, url }); }}
            style={{ width: 92, height: 68, padding: 0, border: "1px solid var(--ct-border-subtle)", borderRadius: 6, overflow: "hidden", background: "var(--ct-bg)", cursor: "zoom-in" }}
          >
            <img src={url} alt={attachment.name} style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} />
          </button>
        ))}
      </div>
      {open && <ImageFullscreen image={{ name: open.name, src: open.url }} anchorRef={galleryRef} onClose={() => setOpen(null)} />}
    </>
  );
}

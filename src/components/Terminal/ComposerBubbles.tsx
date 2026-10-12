import { AttachmentCapsule } from "@codeterm/chat-ui";
import { attachmentKindForPath } from "@codeterm/chat-core";
import { Image as ImageIcon, Paperclip, X } from "lucide-react";

export function AttachmentChip({ path, label, thumbnail, onRemove }: { path: string; label?: string; thumbnail?: string | null; onRemove: () => void }) {
  const name = label || path.split(/[\\/]/).pop() || path;
  const Icon = thumbnail || /\.(avif|bmp|gif|heic|heif|ico|jpe?g|png|svg|tiff?|webp)$/i.test(name) || label ? ImageIcon : Paperclip;
  return (
    <div
      data-testid="attachment-chip"
      className="inline-flex h-7 min-w-0 max-w-full items-center gap-1.5 rounded-full px-2.5 text-xs font-mono"
      style={{
        background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
        color: "var(--ct-fg, #c9d1d9)",
        border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 35%, transparent)",
      }}
    >
      {thumbnail
        ? <img data-testid="attachment-chip-thumb" src={thumbnail} alt="" className="-ml-1 h-5 w-5 shrink-0 rounded-[5px] object-cover" />
        : <Icon size={12} aria-hidden="true" className="shrink-0 opacity-70" />}
      <span className="min-w-0 max-w-[200px] truncate" title={path}>{name}</span>
      <button
        type="button"
        onClick={onRemove}
        className="shrink-0 rounded-full opacity-60 hover:opacity-100 focus-visible:opacity-100 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
        style={{ background: "transparent", border: "none", padding: 0, cursor: "pointer", color: "inherit" }}
        title="Remove"
        aria-label={`Remove ${name}`}
      >
        <X size={12} aria-hidden="true" />
      </button>
    </div>
  );
}

export function ComposerBubbleRow({
  attachments,
  onRemove,
}: {
  attachments: string[];
  onRemove: (path: string) => void;
}) {
  if (attachments.length === 0) return null;
  const visible = attachments.slice(0, 4);
  const overflow = attachments.length - visible.length;
  return (
    <div
      data-testid="composer-bubble-row"
      className="contents"
    >
      {visible.map((p) => (
        <span key={p} className="inline-flex items-center gap-1">
          {attachmentKindForPath(p) === "image"
            ? <AttachmentCapsule attachment={{ path: p, kind: "image" }} />
            : <AttachmentChip path={p} onRemove={() => onRemove(p)} />}
          {attachmentKindForPath(p) === "image" && <button type="button" aria-label={`Remove ${p.split(/[\\/]/).pop()}`} onClick={() => onRemove(p)}><X size={12} aria-hidden /></button>}
        </span>
      ))}
      {overflow > 0 && (
        <span
          data-testid="attachment-overflow-bubble"
          className="inline-flex h-7 items-center rounded-full border px-2 text-xs font-mono"
          style={{
            background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 10%, transparent)",
            borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
            color: "var(--ct-muted-fg)",
          }}
        >
          and {overflow} more
        </span>
      )}
    </div>
  );
}


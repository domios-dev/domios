import { Clock3, Image as ImageIcon, Paperclip, X } from "lucide-react";

function AttachmentChip({ path, onRemove }: { path: string; onRemove: () => void }) {
  const name = path.split(/[\\/]/).pop() || path;
  const Icon = /\.(avif|bmp|gif|heic|heif|ico|jpe?g|png|svg|tiff?|webp)$/i.test(name) ? ImageIcon : Paperclip;
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
      <Icon size={12} aria-hidden="true" className="shrink-0 opacity-70" />
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
  queuedCount,
  failedCount,
  onOpenDelayedQueue,
}: {
  attachments: string[];
  onRemove: (path: string) => void;
  queuedCount: number;
  failedCount: number;
  onOpenDelayedQueue: () => void;
}) {
  if (attachments.length === 0 && queuedCount === 0 && failedCount === 0) return null;
  const visible = attachments.slice(0, 4);
  const overflow = attachments.length - visible.length;
  return (
    <div
      data-testid="composer-bubble-row"
      className="contents"
    >
      {visible.map((p) => (
        <AttachmentChip key={p} path={p} onRemove={() => onRemove(p)} />
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
      {(queuedCount > 0 || failedCount > 0) && (
        <button
          type="button"
          data-testid="delayed-send-bubble"
          onMouseDown={(e) => e.preventDefault()}
          onClick={onOpenDelayedQueue}
          className="inline-flex h-7 items-center gap-1 rounded-full border px-2 text-xs font-mono hover:brightness-110"
          style={{
            background: failedCount > 0
              ? "color-mix(in srgb, var(--ct-red, #f85149) 12%, transparent)"
              : "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
            borderColor: failedCount > 0
              ? "color-mix(in srgb, var(--ct-red, #f85149) 34%, transparent)"
              : "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
            color: failedCount > 0 ? "var(--ct-red, #f85149)" : "var(--ct-muted-fg)",
          }}
          title="Queued messages"
        >
          <Clock3 size={12} />
          <span>{queuedCount > 0 ? `${queuedCount} queued` : `${failedCount} failed`}</span>
        </button>
      )}
    </div>
  );
}


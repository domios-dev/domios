import { Paperclip } from "lucide-react";

const btnBase =
  "ct-liquid-interactive flex items-center justify-center shrink-0";

export function ComposerAttachButton({
  uploading,
  onClick,
}: {
  uploading: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      data-testid="composer-attach-button"
      title="Attach file"
      disabled={uploading}
      onClick={onClick}
      className={`${btnBase} ct-composer-round-button ct-composer-inner-button h-8 w-8 rounded-full border shadow-none disabled:opacity-40`}
      style={{
        background: "var(--ct-surface, #161b22)",
        border: "1px solid var(--ct-border)",
        color: "var(--ct-fg, #c9d1d9)",
      }}
    >
      <Paperclip size={16} strokeWidth={2.3} className={uploading ? "animate-pulse" : undefined} />
    </button>
  );
}

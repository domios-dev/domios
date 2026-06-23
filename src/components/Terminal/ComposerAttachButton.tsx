import { Paperclip } from "lucide-react";

const btnBase =
  "flex items-center justify-center shrink-0 transition-all duration-150";

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
      className={`${btnBase} ct-composer-round-button h-10 w-10 rounded-full border shadow-none hover:scale-[1.03] disabled:opacity-40`}
      style={{
        background: "var(--ct-surface, #161b22)",
        border: "1px solid var(--ct-border)",
        color: "var(--ct-fg, #c9d1d9)",
      }}
    >
      <Paperclip size={20} strokeWidth={2.3} className={uploading ? "animate-pulse" : undefined} />
    </button>
  );
}

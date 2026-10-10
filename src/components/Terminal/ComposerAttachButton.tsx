import { Paperclip } from "lucide-react";
import { COMPOSER_BUTTON_BASE } from "./composerButton";

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
      className={`${COMPOSER_BUTTON_BASE} ct-composer-round-button ct-composer-control rounded-full border shadow-none disabled:opacity-40`}
      style={{
        background: "var(--ct-surface, #161b22)",
        border: "1px solid var(--ct-border)",
        color: "var(--ct-fg, #c9d1d9)",
      }}
    >
      <Paperclip size={17} strokeWidth={2.2} className={uploading ? "animate-pulse" : undefined} />
    </button>
  );
}

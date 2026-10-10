import { X } from "lucide-react";

const message = "Terminal input failed. Check the connection.";

export function InputWriteFailureLine({ onDismiss }: { onDismiss: () => void }) {
  return (
    <div role="status" aria-label={message} title={message}
      className="flex min-w-0 shrink-0 items-center gap-1 px-2 text-xs leading-4"
      style={{ color: "var(--ct-muted-fg)" }}>
      <span aria-hidden="true" style={{ color: "var(--ct-danger)" }}>·</span>
      <span className="min-w-0 flex-1 truncate">{message}</span>
      <button type="button" aria-label="Dismiss terminal input status" onClick={onDismiss}
        className="shrink-0" style={{ color: "var(--ct-muted-fg)" }}>
        <X size={12} aria-hidden="true" />
      </button>
    </div>
  );
}

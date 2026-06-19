/**
 * Generic confirm dialog. Presentational and content-agnostic: the caller owns
 * the title, the message body, and the button labels. Used by the chatBackend
 * model picker (P6) to surface a plugin-supplied "this switch is disruptive"
 * warning — but holds no provider/model knowledge of its own.
 */

export function ConfirmDialog({
  title,
  message,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  testId = "confirm-dialog",
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  testId?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div
      role="dialog"
      aria-modal="true"
      data-testid={testId}
      onMouseDown={(e) => e.preventDefault()}
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0,0,0,0.5)",
        zIndex: 60,
      }}
    >
      <div
        style={{
          width: 440,
          maxWidth: "92vw",
          background: "var(--ct-bg-elev, #1e1e2e)",
          color: "var(--ct-fg)",
          border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
          borderRadius: 12,
          boxShadow: "0 16px 48px rgba(0,0,0,0.55)",
          padding: "20px 22px",
        }}
      >
        <div style={{ fontSize: 16, fontWeight: 620, marginBottom: 10 }}>{title}</div>
        <p
          data-testid={`${testId}-message`}
          style={{ fontSize: 13, color: "var(--ct-muted)", margin: "0 0 18px", lineHeight: 1.5, whiteSpace: "pre-wrap" }}
        >
          {message}
        </p>
        <div style={{ display: "flex", gap: 9, justifyContent: "flex-end" }}>
          <button
            type="button"
            data-testid={`${testId}-cancel`}
            onClick={onCancel}
            style={{
              fontSize: 12,
              borderRadius: 4,
              padding: "6px 12px",
              border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
              background: "transparent",
              color: "var(--ct-fg)",
              cursor: "pointer",
            }}
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            data-testid={`${testId}-confirm`}
            onClick={onConfirm}
            style={{
              fontSize: 12,
              borderRadius: 4,
              padding: "6px 12px",
              border: "1px solid var(--ct-accent, #6366f1)",
              background: "var(--ct-accent, #6366f1)",
              color: "#fff",
              fontWeight: 560,
              cursor: "pointer",
            }}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

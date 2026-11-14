import type { CSSProperties } from "react";
import { Plus } from "lucide-react";
import type { ChannelFormInput } from "./channelForm";

/** Plugin and team channels share one look: a name with its Official/Custom badge, and one inline add form. */
export function ChannelBadge({ official, testId }: { official: boolean; testId?: string }) {
  return (
    <span
      data-testid={testId}
      style={{
        fontSize: 9.5,
        fontFamily: '"JetBrains Mono", ui-monospace, monospace',
        letterSpacing: 0,
        textTransform: "uppercase",
        borderRadius: 4,
        padding: "1px 5px",
        flex: "none",
        color: "var(--ct-muted)",
        border: "1px solid var(--ct-border-default)",
        background: official ? "var(--ct-bg-active)" : "transparent",
      }}
    >
      {official ? "Official" : "Custom"}
    </span>
  );
}

export function ChannelName({ label, official, id, badgeTestId }: { label: string; official: boolean; id?: string; badgeTestId?: string }) {
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
      <span id={id} style={{ fontSize: 12.5, fontWeight: 560, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span>
      <ChannelBadge official={official} testId={badgeTestId} />
    </span>
  );
}

export function ChannelAddForm({
  testIdPrefix,
  nameRequired,
  busy,
  open,
  error = null,
  onOpenChange,
  onAdd,
}: {
  testIdPrefix: string;
  nameRequired: boolean;
  busy: boolean;
  open: boolean;
  /** Why the last add failed, shown inside the form. */
  error?: string | null;
  onOpenChange: (open: boolean) => void;
  onAdd: (input: ChannelFormInput) => Promise<boolean>;
}) {
  if (!open) {
    return (
      <button type="button" data-testid={`${testIdPrefix}-add-open`} onClick={() => onOpenChange(true)} style={addButtonStyle}>
        <Plus size={14} strokeWidth={1.8} aria-hidden="true" />
        Add channel
      </button>
    );
  }
  return (
    <form
      style={{ display: "flex", flexDirection: "column", gap: 8 }}
      data-testid={`${testIdPrefix}-add-form`}
      aria-label="Add channel"
      onSubmit={async (event) => {
        event.preventDefault();
        const form = event.currentTarget;
        const data = new FormData(form);
        const ok = await onAdd({
          name: String(data.get("name") ?? ""),
          url: String(data.get("url") ?? ""),
        });
        if (!ok) return;
        form.reset();
      }}
    >
      <input data-testid={`${testIdPrefix}-name-input`} name="name" aria-label={nameRequired ? "Channel name" : "Channel name (optional)"}
        required={nameRequired} autoFocus placeholder={nameRequired ? "channel name" : "name (optional)"} style={inputStyle} />
      <input data-testid={`${testIdPrefix}-url-input`} name="url" aria-label="Channel URL" required placeholder="channel URL" style={inputStyle} />
      {error && (
        <div role="alert" data-testid={`${testIdPrefix}-add-error`} style={{ fontSize: 11.5, lineHeight: 1.4, color: "var(--ct-err)", overflowWrap: "anywhere" }}>
          {error}
        </div>
      )}
      <div style={{ display: "flex", gap: 8 }}>
        <button type="submit" data-testid={`${testIdPrefix}-add-submit`} disabled={busy} style={{ ...smallButtonStyle, flex: 1 }}>
          Add
        </button>
        <button type="button" data-testid={`${testIdPrefix}-add-cancel`} onClick={() => onOpenChange(false)} style={{ ...smallButtonStyle, flex: 1 }}>
          Cancel
        </button>
      </div>
    </form>
  );
}

const addButtonStyle: CSSProperties = {
  width: "100%",
  minHeight: 34,
  borderRadius: 7,
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "transparent",
  color: "var(--ct-fg)",
  cursor: "pointer",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 7,
  fontSize: 12,
};

const smallButtonStyle: CSSProperties = {
  fontSize: 11.5,
  borderRadius: 6,
  padding: "6px 10px",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "transparent",
  color: "var(--ct-fg)",
  cursor: "pointer",
};

const inputStyle: CSSProperties = {
  fontSize: 12,
  borderRadius: 6,
  padding: "7px 9px",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "var(--ct-bg-elev, rgba(255,255,255,0.03))",
  color: "var(--ct-fg)",
};

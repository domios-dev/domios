// Registered-channel management — kept compact in a collapsible footer so it's
// available without dominating the list/detail layout.
import { useState } from "react";
import { Plus } from "lucide-react";

export function ChannelManager({
  busy,
  onAction,
  open,
  onOpenChange,
}: {
  busy: string | null;
  onAction: (key: string, action: Record<string, unknown>) => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");

  return (
    <div style={{ borderTop: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))", paddingTop: 10, paddingBottom: 16, marginTop: 10 }}>
      {!open ? (
        <button
          type="button"
          onClick={() => onOpenChange(true)}
          style={{
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
          }}
        >
          <Plus size={14} strokeWidth={1.8} />
          Add channel
        </button>
      ) : (
        <div>
          <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="channel URL (git or local path)"
              style={input}
            />
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="name (optional)" style={input} />
            <div style={{ display: "flex", gap: 6 }}>
              <button
                disabled={!url.trim() || busy === "add-channel"}
                onClick={() => {
                  onAction("add-channel", { kind: "add-channel", url: url.trim(), name: name.trim() || undefined });
                  setUrl("");
                  setName("");
                  onOpenChange(false);
                }}
                style={{ flex: 1, ...btn }}
              >
                Add
              </button>
              <button
                type="button"
                onClick={() => onOpenChange(false)}
                style={{ flex: 1, ...btn }}
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

const btn: React.CSSProperties = {
  fontSize: 11,
  borderRadius: 4,
  padding: "3px 9px",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "transparent",
  color: "var(--ct-fg)",
  cursor: "pointer",
};

const input: React.CSSProperties = {
  fontSize: 11.5,
  borderRadius: 4,
  padding: "5px 8px",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "var(--ct-bg-elev, rgba(255,255,255,0.03))",
  color: "var(--ct-fg)",
};

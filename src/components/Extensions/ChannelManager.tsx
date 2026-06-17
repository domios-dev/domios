// Registered-channel management — kept compact in a collapsible footer so it's
// available without dominating the list/detail layout.
import { useState } from "react";
import type { ChannelRecord } from "./types";
import { GroupLabel } from "./shared";

export function ChannelManager({
  channels,
  busy,
  onAction,
}: {
  channels: ChannelRecord[];
  busy: string | null;
  onAction: (key: string, action: Record<string, unknown>) => void;
}) {
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");

  return (
    <div style={{ borderTop: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))", paddingTop: 10, marginTop: 10 }}>
      <button
        onClick={() => setOpen((v) => !v)}
        style={{ background: "transparent", border: "none", color: "var(--ct-muted)", cursor: "pointer", fontSize: 11.5, padding: 0, display: "flex", alignItems: "center", gap: 6 }}
      >
        <span style={{ transform: open ? "rotate(90deg)" : "none", transition: "transform 0.12s" }}>▸</span>
        Channels ({channels.length})
      </button>

      {open && (
        <div style={{ marginTop: 10 }}>
          <div style={{ display: "flex", gap: 6, marginBottom: 8 }}>
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="channel URL (git or local path)"
              style={{ flex: 1, ...input }}
            />
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="name (optional)" style={{ width: 120, ...input }} />
            <button
              disabled={!url.trim() || busy === "add-channel"}
              onClick={() => {
                onAction("add-channel", { kind: "add-channel", url: url.trim(), name: name.trim() || undefined });
                setUrl("");
                setName("");
              }}
              style={btn}
            >
              Add
            </button>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            {channels.map((c) => (
              <div key={c.name} style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 10px", borderRadius: 7, border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))" }}>
                <span style={{ width: 6, height: 6, borderRadius: "50%", background: "#22c55e", flex: "none" }} />
                <span style={{ fontSize: 12 }}>{c.name}</span>
                <span style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 9.5, color: "var(--ct-muted)", marginLeft: "auto", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 200 }}>
                  {c.kind} · {c.url}
                </span>
                <button disabled={busy === `refresh:${c.name}`} onClick={() => onAction(`refresh:${c.name}`, { kind: "refresh-channel", channel: c.name })} style={btn}>
                  Refresh
                </button>
              </div>
            ))}
            {channels.length === 0 && <GroupLabel>No channels configured.</GroupLabel>}
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

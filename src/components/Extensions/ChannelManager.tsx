// Registered-channel management — kept compact in a collapsible footer so it's
// available without dominating the list/detail layout.
import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import type { ChannelRecord } from "./types";

const CORE_CHANNEL = "codeterm-core";
const MARKETPLACE_CHANNEL = "codeterm-plugins";

export function ChannelManager({
  busy,
  onAction,
  channels,
  installedCounts,
  onRemoveChannel,
  open,
  onOpenChange,
}: {
  busy: string | null;
  onAction: (key: string, action: Record<string, unknown>) => Promise<boolean>;
  channels: ChannelRecord[];
  installedCounts: Map<string, number>;
  onRemoveChannel: (channel: string, installedCount: number) => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");

  return (
    <div style={{ borderTop: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))", paddingTop: 10, paddingBottom: 16, marginTop: 10, display: "flex", flexDirection: "column", gap: 8 }} data-testid="extensions-channel-manager">
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
        <div style={{ fontSize: 10.5, fontWeight: 650, color: "var(--ct-muted)", textTransform: "uppercase" }}>Channels</div>
        <div style={{ fontSize: 10.5, color: "var(--ct-muted)" }}>{channels.length}</div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 5 }} data-testid="extensions-channel-list">
        {channels.map((channel) => {
          const removable = channel.removable !== false && channel.system !== true;
          const installedCount = installedCounts.get(channel.name) ?? 0;
          return (
            <div
              key={channel.name}
              data-testid="extensions-channel-row"
              data-channel-name={channel.name}
              style={{
                display: "grid",
                gridTemplateColumns: removable ? "1fr auto" : "1fr",
                gap: 6,
                alignItems: "center",
                border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))",
                borderRadius: 7,
                padding: "6px 7px",
                background: "var(--ct-bg-elev, rgba(255,255,255,0.025))",
              }}
            >
              <div style={{ minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
                  <span style={{ fontSize: 11.5, color: "var(--ct-fg)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{channel.name}</span>
                  <span style={badgeStyle}>{channelBadge(channel)}</span>
                </div>
                <div title={channel.url} style={{ fontSize: 10.5, color: "var(--ct-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", marginTop: 2 }}>{channel.url}</div>
              </div>
              {removable && (
                <button
                  type="button"
                  data-testid="extensions-channel-remove"
                  data-channel-name={channel.name}
                  disabled={busy === `remove-channel:${channel.name}`}
                  title={`Remove ${channel.name}`}
                  onClick={() => onRemoveChannel(channel.name, installedCount)}
                  style={{
                    width: 24,
                    height: 24,
                    borderRadius: 6,
                    border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
                    background: "transparent",
                    color: "var(--ct-muted)",
                    cursor: "pointer",
                    display: "grid",
                    placeItems: "center",
                  }}
                >
                  <Trash2 size={13} />
                </button>
              )}
            </div>
          );
        })}
        {channels.length === 0 && <div style={{ color: "var(--ct-muted)", fontSize: 11.5 }}>No channels registered.</div>}
      </div>

      {!open ? (
        <button
          type="button"
          data-testid="extensions-channel-add-open"
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
              data-testid="extensions-channel-url-input"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="channel URL (git or local path)"
              style={input}
            />
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="name (optional)" style={input} data-testid="extensions-channel-name-input" />
            <div style={{ display: "flex", gap: 6 }}>
              <button
                data-testid="extensions-channel-add-submit"
                disabled={!url.trim() || busy !== null}
                onClick={async () => {
                  const added = await onAction("add-channel", { kind: "add-channel", url: url.trim(), name: name.trim() || undefined });
                  if (!added) return;
                  await onAction("refresh-channel", { kind: "refresh-channel" });
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
                data-testid="extensions-channel-add-cancel"
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

function channelBadge(channel: ChannelRecord): string {
  if (channel.name === CORE_CHANNEL || channel.system) return "core";
  if (channel.name === MARKETPLACE_CHANNEL) return "marketplace";
  return channel.kind;
}

const badgeStyle: React.CSSProperties = {
  flex: "none",
  fontSize: 9.5,
  lineHeight: "15px",
  padding: "0 5px",
  borderRadius: 999,
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  color: "var(--ct-muted)",
  textTransform: "uppercase",
};

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

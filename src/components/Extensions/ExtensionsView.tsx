/**
 * Extensions — first-class surface listing plugins, agents, and system extensions
 * as cards (variant C). Promotes the buried Settings → Plugins data to a top-level
 * view and subsumes it: capability chips (signature element), pin-to-top, and a
 * marketplace (channels + available/updates). Backend: `plugin_manager_snapshot` /
 * `plugin_manager_action` (web-mirrored via bridge HTTP_MAP).
 *
 * Pinning is stored in the generic `config.plugins.<id>.pinned` bag — no Rust
 * schema change needed.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "../../lib/bridge";
import { CapabilityChip, capabilityLabel } from "./CapabilityChip";

interface PluginCapabilities {
  agent_provider?: boolean;
  secret_backend?: boolean;
  transcriber?: boolean;
}
interface SettingField {
  type: "select" | "text" | "toggle" | "number";
  key: string;
  label: string;
  description?: string;
  placeholder?: string;
  options?: { value: string; label: string }[];
}
interface InstalledPlugin {
  id: string;
  version?: string | null;
  display_name?: string | null;
  state?: string;
  system?: boolean;
  capabilities?: PluginCapabilities | null;
  granted_permissions?: string[] | null;
  settings_schema?: SettingField[] | null;
}
interface ChannelRecord {
  name: string;
  url: string;
  kind: "local" | "git";
  system?: boolean;
}
interface DiffRow {
  channel: string;
  id: string;
  local_version?: string | null;
  remote_version?: string | null;
  drift: string;
  compatible?: boolean;
}
interface PluginSnapshot {
  plugins: InstalledPlugin[];
  channels: ChannelRecord[];
  diff: DiffRow[];
}

type Tab = "installed" | "marketplace";
type Group = "pinned" | "agents" | "plugins" | "system";

function groupOf(p: InstalledPlugin, pinned: Set<string>): Group {
  if (pinned.has(p.id)) return "pinned";
  if (p.system) return "system";
  if (p.capabilities?.agent_provider) return "agents";
  return "plugins";
}
function kindLabel(p: InstalledPlugin): string {
  if (p.system) return "system";
  if (p.capabilities?.secret_backend) return "secret backend";
  if (p.capabilities?.transcriber) return "transcriber";
  if (p.capabilities?.agent_provider) return "agent";
  return "plugin";
}
function iconFor(p: InstalledPlugin): string {
  if (p.capabilities?.transcriber) return "🎙";
  if (p.capabilities?.secret_backend) return "🔐";
  if (p.capabilities?.agent_provider) return "🤖";
  if (p.system) return "⏻";
  return "🧩";
}
const GROUP_TITLES: Record<Group, string> = {
  pinned: "Pinned",
  agents: "Agents",
  plugins: "Plugins",
  system: "System",
};

interface AppConfigShape {
  plugins?: Record<string, { pinned?: boolean; [k: string]: unknown } | undefined>;
  [k: string]: unknown;
}

export function ExtensionsView({ visible }: { visible: boolean }) {
  const [snapshot, setSnapshot] = useState<PluginSnapshot>({ plugins: [], channels: [], diff: [] });
  const [config, setConfig] = useState<AppConfigShape | null>(null);
  const [tab, setTab] = useState<Tab>("installed");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const saveSetting = useCallback(async (pluginId: string, key: string, value: unknown) => {
    const cfg = await invoke<AppConfigShape>("get_config");
    const plugins = { ...(cfg.plugins ?? {}) };
    plugins[pluginId] = { ...(plugins[pluginId] ?? {}), [key]: value };
    await invoke("update_config", { config: { ...cfg, plugins } });
    setConfig({ ...cfg, plugins });
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [snap, cfg] = await Promise.all([
        invoke<PluginSnapshot>("plugin_manager_snapshot"),
        invoke<AppConfigShape>("get_config"),
      ]);
      setSnapshot({ plugins: snap?.plugins ?? [], channels: snap?.channels ?? [], diff: snap?.diff ?? [] });
      setConfig(cfg ?? {});
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (visible) void refresh();
  }, [visible, refresh]);

  const pinned = useMemo(() => {
    const s = new Set<string>();
    for (const [id, p] of Object.entries(config?.plugins ?? {})) if (p?.pinned) s.add(id);
    return s;
  }, [config]);

  const togglePin = useCallback(
    async (id: string) => {
      const cfg = await invoke<AppConfigShape>("get_config");
      const plugins = { ...(cfg.plugins ?? {}) };
      const entry = { ...(plugins[id] ?? {}) };
      entry.pinned = !entry.pinned;
      plugins[id] = entry;
      await invoke("update_config", { config: { ...cfg, plugins } });
      setConfig({ ...cfg, plugins });
    },
    [],
  );

  const runAction = useCallback(
    async (key: string, action: Record<string, unknown>) => {
      setBusy(key);
      try {
        const snap = await invoke<PluginSnapshot>("plugin_manager_action", { action });
        setSnapshot({ plugins: snap?.plugins ?? [], channels: snap?.channels ?? [], diff: snap?.diff ?? [] });
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(null);
      }
    },
    [],
  );

  const grouped = useMemo(() => {
    const out: Record<Group, InstalledPlugin[]> = { pinned: [], agents: [], plugins: [], system: [] };
    for (const p of snapshot.plugins) out[groupOf(p, pinned)].push(p);
    return out;
  }, [snapshot, pinned]);

  const selected = selectedId ? snapshot.plugins.find((p) => p.id === selectedId) ?? null : null;

  return (
    <div className="h-full overflow-auto" style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
      <div style={{ maxWidth: 860, margin: "0 auto", padding: "28px 24px" }}>
        <header style={{ marginBottom: 18 }}>
          <h1 style={{ fontSize: 22, fontWeight: 640, margin: 0, letterSpacing: "-0.01em" }}>Extensions</h1>
          <p style={{ color: "var(--ct-muted)", fontSize: 13, marginTop: 6 }}>
            Plugins, agents, and system extensions — each defined by what it's allowed to do.
          </p>
        </header>

        {selected ? (
          <PluginDetail
            plugin={selected}
            config={config}
            onBack={() => setSelectedId(null)}
            onSave={saveSetting}
          />
        ) : (
        <>
        <div style={{ display: "flex", gap: 4, marginBottom: 18, borderBottom: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))" }}>
          {(["installed", "marketplace"] as Tab[]).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              style={{
                background: "transparent",
                border: "none",
                borderBottom: tab === t ? "2px solid var(--ct-accent, #6366f1)" : "2px solid transparent",
                color: tab === t ? "var(--ct-fg)" : "var(--ct-muted)",
                padding: "6px 10px",
                fontSize: 13,
                cursor: "pointer",
                textTransform: "capitalize",
              }}
            >
              {t}
            </button>
          ))}
        </div>

        {error && (
          <div style={{ fontSize: 12.5, color: "var(--ct-muted)", border: "1px solid var(--ct-border-default, rgba(255,255,255,0.1))", borderRadius: 8, padding: "10px 12px", marginBottom: 16 }}>
            Plugin data unavailable: {error}
          </div>
        )}

        {tab === "installed" ? (
          <>
            {(["pinned", "system", "agents", "plugins"] as Group[]).map((g) =>
              grouped[g].length === 0 ? null : (
                <section key={g} style={{ marginBottom: 22 }}>
                  <GroupLabel>{GROUP_TITLES[g]}</GroupLabel>
                  <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                    {grouped[g].map((p) => (
                      <PluginCard
                        key={p.id}
                        plugin={p}
                        pinned={pinned.has(p.id)}
                        onTogglePin={() => togglePin(p.id)}
                        onOpen={() => setSelectedId(p.id)}
                      />
                    ))}
                  </div>
                </section>
              ),
            )}
            {snapshot.plugins.length === 0 && !error && (
              <div style={{ color: "var(--ct-muted)", fontSize: 13 }}>No extensions installed yet.</div>
            )}
          </>
        ) : (
          <Marketplace snapshot={snapshot} busy={busy} onAction={runAction} />
        )}
        </>
        )}
      </div>
    </div>
  );
}

function GroupLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontFamily: '"JetBrains Mono", ui-monospace, monospace',
        fontSize: 10.5,
        letterSpacing: "0.1em",
        textTransform: "uppercase",
        color: "var(--ct-muted)",
        margin: "0 0 8px 2px",
      }}
    >
      {children}
    </div>
  );
}

function PluginCard({
  plugin,
  pinned,
  onTogglePin,
  onOpen,
}: {
  plugin: InstalledPlugin;
  pinned: boolean;
  onTogglePin: () => void;
  onOpen: () => void;
}) {
  const caps = plugin.granted_permissions ?? [];
  const active = plugin.state === "active" || plugin.state === "Active";
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onOpen();
      }}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "11px 14px",
        borderRadius: 10,
        border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))",
        background: "var(--ct-bg-elev, rgba(255,255,255,0.02))",
        cursor: "pointer",
      }}
    >
      <div style={{ width: 32, height: 32, borderRadius: 8, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 16, flex: "none", background: "rgba(255,255,255,0.04)" }}>
        {iconFor(plugin)}
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 560, display: "flex", alignItems: "center", gap: 7 }}>
          {plugin.display_name || plugin.id}
          <span style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 9.5, color: "var(--ct-muted)", border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))", borderRadius: 3, padding: "0 5px", fontWeight: 400 }}>
            {kindLabel(plugin)}
          </span>
        </div>
        <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10.5, color: "var(--ct-muted)", marginTop: 2 }}>
          {plugin.version ? `v${plugin.version}` : ""}
        </div>
        {caps.length > 0 && (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5, marginTop: 8 }}>
            {caps.map((c) => (
              <CapabilityChip key={c} label={c} title={capabilityLabel(c)} />
            ))}
          </div>
        )}
      </div>
      <button
        onClick={(e) => {
          e.stopPropagation();
          onTogglePin();
        }}
        title={pinned ? "Unpin from titlebar" : "Pin to titlebar"}
        style={{
          background: "transparent",
          border: "none",
          cursor: "pointer",
          fontSize: 14,
          color: pinned ? "var(--ct-accent, #6366f1)" : "var(--ct-muted)",
          transform: pinned ? "rotate(45deg)" : "none",
          padding: 2,
        }}
      >
        📌
      </button>
      <span
        title={active ? "active" : "inactive"}
        style={{ width: 7, height: 7, borderRadius: "50%", flex: "none", background: active ? "#22c55e" : "transparent", border: active ? "none" : "1.5px solid var(--ct-muted)", boxShadow: active ? "0 0 6px rgba(34,197,94,0.5)" : "none" }}
      />
    </div>
  );
}

function PluginDetail({
  plugin,
  config,
  onBack,
  onSave,
}: {
  plugin: InstalledPlugin;
  config: AppConfigShape | null;
  onBack: () => void;
  onSave: (pluginId: string, key: string, value: unknown) => Promise<void>;
}) {
  const current = (config?.plugins?.[plugin.id] ?? {}) as Record<string, unknown>;
  const schema = plugin.settings_schema ?? [];
  const caps = plugin.granted_permissions ?? [];
  return (
    <div>
      <button
        onClick={onBack}
        style={{ background: "transparent", border: "none", color: "var(--ct-muted)", cursor: "pointer", fontSize: 12.5, padding: "0 0 14px" }}
      >
        ← Extensions
      </button>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 18 }}>
        <div style={{ width: 40, height: 40, borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 20, background: "rgba(255,255,255,0.04)" }}>
          {iconFor(plugin)}
        </div>
        <div>
          <div style={{ fontSize: 16, fontWeight: 620 }}>{plugin.display_name || plugin.id}</div>
          <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10.5, color: "var(--ct-muted)" }}>
            {kindLabel(plugin)}
            {plugin.version ? ` · v${plugin.version}` : ""}
          </div>
        </div>
      </div>

      <GroupLabel>Settings</GroupLabel>
      {schema.length === 0 ? (
        <div style={{ color: "var(--ct-muted)", fontSize: 12.5, marginBottom: 20 }}>This plugin has no settings.</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 14, marginBottom: 22 }}>
          {schema.map((f) => (
            <SettingFieldRow
              key={f.key}
              field={f}
              value={current[f.key]}
              onChange={(v) => void onSave(plugin.id, f.key, v)}
            />
          ))}
        </div>
      )}

      {caps.length > 0 && (
        <>
          <GroupLabel>Granted permissions</GroupLabel>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
            {caps.map((c) => (
              <CapabilityChip key={c} label={c} title={capabilityLabel(c)} />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function SettingFieldRow({
  field,
  value,
  onChange,
}: {
  field: SettingField;
  value: unknown;
  onChange: (v: unknown) => void;
}) {
  const labelEl = (
    <div style={{ fontSize: 12.5, fontWeight: 540, marginBottom: 4 }}>
      {field.label}
      {field.description && (
        <span style={{ display: "block", fontSize: 11, color: "var(--ct-muted)", fontWeight: 400, marginTop: 2 }}>
          {field.description}
        </span>
      )}
    </div>
  );
  const inputStyle: React.CSSProperties = {
    width: "100%",
    background: "var(--ct-bg-elev, rgba(255,255,255,0.03))",
    border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
    borderRadius: 6,
    padding: "6px 9px",
    color: "var(--ct-fg)",
    fontSize: 12.5,
  };
  if (field.type === "toggle") {
    return (
      <label style={{ display: "flex", alignItems: "center", gap: 9, cursor: "pointer" }}>
        <input type="checkbox" checked={!!value} onChange={(e) => onChange(e.target.checked)} />
        {labelEl}
      </label>
    );
  }
  if (field.type === "select") {
    return (
      <div>
        {labelEl}
        <select style={inputStyle} value={String(value ?? "")} onChange={(e) => onChange(e.target.value)}>
          <option value="">—</option>
          {(field.options ?? []).map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </div>
    );
  }
  return (
    <div>
      {labelEl}
      <input
        style={inputStyle}
        type={field.type === "number" ? "number" : "text"}
        value={String(value ?? "")}
        placeholder={field.placeholder}
        onChange={(e) => onChange(field.type === "number" ? Number(e.target.value) : e.target.value)}
      />
    </div>
  );
}

function Marketplace({
  snapshot,
  busy,
  onAction,
}: {
  snapshot: PluginSnapshot;
  busy: string | null;
  onAction: (key: string, action: Record<string, unknown>) => void;
}) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  return (
    <>
      <section style={{ marginBottom: 22 }}>
        <GroupLabel>Channels</GroupLabel>
        <div style={{ display: "flex", gap: 6, marginBottom: 8 }}>
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="channel URL (git or local path)"
            style={{ flex: 1, ...mpInput }}
          />
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="name (optional)"
            style={{ width: 140, ...mpInput }}
          />
          <button
            disabled={!url.trim() || busy === "add-channel"}
            onClick={() => {
              onAction("add-channel", { kind: "add-channel", url: url.trim(), name: name.trim() || undefined });
              setUrl("");
              setName("");
            }}
            style={mpBtn}
          >
            Add
          </button>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {snapshot.channels.map((c) => (
            <div key={c.name} style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderRadius: 8, border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))" }}>
              <span style={{ width: 6, height: 6, borderRadius: "50%", background: "#22c55e", flex: "none" }} />
              <span style={{ fontSize: 12.5 }}>{c.name}</span>
              <span style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10, color: "var(--ct-muted)", marginLeft: "auto" }}>
                {c.kind} · {c.url}
              </span>
              <button
                disabled={busy === `refresh:${c.name}`}
                onClick={() => onAction(`refresh:${c.name}`, { kind: "refresh-channel", channel: c.name })}
                style={mpBtn}
              >
                Refresh
              </button>
            </div>
          ))}
          {snapshot.channels.length === 0 && (
            <div style={{ color: "var(--ct-muted)", fontSize: 12.5 }}>No channels configured.</div>
          )}
        </div>
      </section>

      <section>
        <GroupLabel>Available · updates</GroupLabel>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {snapshot.diff.map((d) => (
            <div key={`${d.channel}:${d.id}`} style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderRadius: 8, border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))" }}>
              <span style={{ fontSize: 12.5 }}>{d.id}</span>
              <span style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10, color: "var(--ct-muted)" }}>
                {d.local_version ?? "—"} → {d.remote_version ?? "—"}
              </span>
              <span style={{ fontSize: 10, color: d.compatible === false ? "#fca5a5" : "var(--ct-muted)", marginLeft: "auto" }}>
                {d.drift}
                {d.compatible === false ? " · incompatible" : ""}
              </span>
              <button
                disabled={busy === `install:${d.id}` || d.compatible === false}
                onClick={() => onAction(`install:${d.id}`, { kind: "install", plugin_id: d.id, channel: d.channel })}
                style={mpBtn}
              >
                {d.local_version ? "Update" : "Install"}
              </button>
            </div>
          ))}
          {snapshot.diff.length === 0 && (
            <div style={{ color: "var(--ct-muted)", fontSize: 12.5 }}>Everything is up to date.</div>
          )}
        </div>
      </section>
    </>
  );
}

const mpBtn: React.CSSProperties = {
  fontSize: 11.5,
  borderRadius: 4,
  padding: "4px 10px",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "transparent",
  color: "var(--ct-fg)",
  cursor: "pointer",
};

const mpInput: React.CSSProperties = {
  fontSize: 12,
  borderRadius: 4,
  padding: "5px 9px",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  background: "var(--ct-bg-elev, rgba(255,255,255,0.03))",
  color: "var(--ct-fg)",
};

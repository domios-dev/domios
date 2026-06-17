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
import { AgentProviderScreen } from "../Settings/AgentsTab";
import { SchemaRenderer } from "../Settings/SchemaRenderer";
import type { AppConfig, ProviderConfig, SettingField } from "../../lib/types";
import { PinIcon } from "../Icons";
import { PluginIcon } from "./PluginIcon";
import { PluginView } from "./PluginView";
import { SELECT_EXTENSION_EVENT } from "../Titlebar/extensionsTrayLogic";

interface PluginCapabilities {
  agent_provider?: boolean;
  secret_backend?: boolean;
  transcriber?: boolean;
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
  icon_svg?: string | null;
  icon_color?: string | null;
  icon_hint?: string | null;
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
const GROUP_TITLES: Record<Group, string> = {
  pinned: "Pinned",
  agents: "Agents",
  plugins: "Plugins",
  system: "System",
};

interface AppConfigShape {
  plugins?: Record<string, { pinned?: boolean; serverUrl?: string; [k: string]: unknown } | undefined>;
  integrations?: {
    secrets?: { backend?: string; default_organization?: string; default_collection?: string };
  };
  api?: { host?: string; port?: number; token?: string };
  [k: string]: unknown;
}

function apiBaseOf(config: AppConfigShape | null): { base: string; headers: Record<string, string> } {
  const api = config?.api;
  const host = api?.host === "0.0.0.0" ? "127.0.0.1" : api?.host ?? "127.0.0.1";
  const base = `http://${host}:${api?.port ?? 7686}`;
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (api?.token) headers.Authorization = `Bearer ${api.token}`;
  return { base, headers };
}

// Secret API errors come back either flat ({message}) or nested ({error:{message}}).
async function readApiError(r: Response, fallback: string): Promise<string> {
  const j = (await r.json().catch(() => null)) as { message?: string; error?: { message?: string } } | null;
  return j?.error?.message ?? j?.message ?? `${fallback} (${r.status})`;
}

export function ExtensionsView({ visible }: { visible: boolean }) {
  const [snapshot, setSnapshot] = useState<PluginSnapshot>({ plugins: [], channels: [], diff: [] });
  const [config, setConfig] = useState<AppConfigShape | null>(null);
  const [tab, setTab] = useState<Tab>("installed");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const saveSetting = useCallback(async (pluginId: string, key: string, value: unknown) => {
    try {
      const cfg = await invoke<AppConfigShape>("get_config");
      const plugins = { ...(cfg.plugins ?? {}) };
      plugins[pluginId] = { ...(plugins[pluginId] ?? {}), [key]: value };
      await invoke("update_config", { config: { ...cfg, plugins } });
      setConfig({ ...cfg, plugins });
      setError(null);
    } catch (e) {
      setError(`Couldn't save ${pluginId} setting: ${String(e)}`);
    }
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

  // Deep-link: the titlebar tray dispatches this to jump straight to a plugin.
  useEffect(() => {
    const onSelect = (e: Event) => {
      const id = (e as CustomEvent<{ id: string | null }>).detail?.id ?? null;
      setSelectedId(id);
    };
    window.addEventListener(SELECT_EXTENSION_EVENT, onSelect);
    return () => window.removeEventListener(SELECT_EXTENSION_EVENT, onSelect);
  }, []);

  const pinned = useMemo(() => {
    const s = new Set<string>();
    for (const [id, p] of Object.entries(config?.plugins ?? {})) if (p?.pinned) s.add(id);
    return s;
  }, [config]);

  const togglePin = useCallback(
    async (id: string) => {
      try {
        const cfg = await invoke<AppConfigShape>("get_config");
        const plugins = { ...(cfg.plugins ?? {}) };
        const entry = { ...(plugins[id] ?? {}) };
        entry.pinned = !entry.pinned;
        plugins[id] = entry;
        await invoke("update_config", { config: { ...cfg, plugins } });
        setConfig({ ...cfg, plugins });
        setError(null);
      } catch (e) {
        setError(`Couldn't ${pinned.has(id) ? "unpin" : "pin"} ${id}: ${String(e)}`);
      }
    },
    [pinned],
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

        {error && (
          <div
            role="alert"
            style={{ fontSize: 12.5, color: "var(--ct-err, #e57373)", border: "1px solid var(--ct-err, rgba(229,115,115,0.4))", borderRadius: 8, padding: "10px 12px", marginBottom: 16 }}
          >
            {error}
          </div>
        )}

        {selected ? (
          <PluginDetail
            plugin={selected}
            config={config}
            onBack={() => setSelectedId(null)}
            onSave={saveSetting}
            onUpdateFullConfig={async (next) => {
              try {
                await invoke("update_config", { config: next });
                setConfig(next as unknown as AppConfigShape);
                setError(null);
              } catch (e) {
                setError(`Couldn't save settings: ${String(e)}`);
              }
            }}
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
        <PluginIcon plugin={plugin} size={18} />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 560 }}>
          {plugin.display_name || plugin.id}
        </div>
        <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10.5, color: "var(--ct-muted)", marginTop: 2 }}>
          {plugin.version ? `v${plugin.version}` : ""}
        </div>
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
          display: "flex",
          alignItems: "center",
          color: pinned ? "var(--ct-accent, #6366f1)" : "var(--ct-muted)",
          padding: 4,
        }}
      >
        <PinIcon size={14} filled={pinned} />
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
  onUpdateFullConfig,
}: {
  plugin: InstalledPlugin;
  config: AppConfigShape | null;
  onBack: () => void;
  onSave: (pluginId: string, key: string, value: unknown) => Promise<void>;
  onUpdateFullConfig: (next: AppConfig) => void | Promise<void>;
}) {
  const current = (config?.plugins?.[plugin.id] ?? {}) as Record<string, unknown>;
  const schema = plugin.settings_schema ?? [];
  const caps = plugin.granted_permissions ?? [];
  // SchemaRenderer emits one key per change; fan out to the per-key saver.
  const onPluginConfigChange = (id: string, patch: Partial<ProviderConfig>) => {
    for (const [k, v] of Object.entries(patch)) void onSave(id, k, v);
  };
  const backBtn = (
    <button
      onClick={onBack}
      style={{ background: "transparent", border: "none", color: "var(--ct-muted)", cursor: "pointer", fontSize: 12.5, padding: "0 0 14px" }}
    >
      ← Extensions
    </button>
  );

  // Agent plugins render their real provider settings (descriptor schema, real
  // icon) via the shared AgentProviderScreen — reads/writes config.agents.<id>.
  if (plugin.capabilities?.agent_provider && config) {
    return (
      <div>
        {backBtn}
        <AgentProviderScreen
          providerId={plugin.id}
          fullConfig={config as unknown as AppConfig}
          onUpdateFullConfig={(next) => void onUpdateFullConfig(next)}
          onProviderGone={onBack}
        />
      </div>
    );
  }

  // Secret-backend plugins (Bitwarden) host secret-store management here: a
  // "use as default" toggle (not a picker — installing the plugin lets it BECOME
  // the store), server URL (plugin config), and unlock/status.
  if (plugin.capabilities?.secret_backend && config) {
    return (
      <div>
        {backBtn}
        <SecretBackendDetail plugin={plugin} config={config} />
      </div>
    );
  }

  return (
    <div>
      {backBtn}
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 18 }}>
        <div style={{ width: 40, height: 40, borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 20, background: "rgba(255,255,255,0.04)" }}>
          <PluginIcon plugin={plugin} size={18} />
        </div>
        <div>
          <div style={{ fontSize: 16, fontWeight: 620 }}>{plugin.display_name || plugin.id}</div>
          <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10.5, color: "var(--ct-muted)" }}>
            {kindLabel(plugin)}
            {plugin.version ? ` · v${plugin.version}` : ""}
          </div>
        </div>
      </div>

      {schema.length === 0 ? (
        <>
          <GroupLabel>Settings</GroupLabel>
          <div style={{ color: "var(--ct-muted)", fontSize: 12.5, marginBottom: 20 }}>This plugin has no settings.</div>
        </>
      ) : (
        <div style={{ marginBottom: 22 }}>
          <SchemaRenderer
            schema={schema}
            providerId={plugin.id}
            config={(config ?? {}) as unknown as AppConfig}
            values={current}
            onChange={onPluginConfigChange}
            hidePathLinks
          />
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

interface SecretStatus {
  backend: string;
  status: { status: "logged_out" | "locked" | "unlocked"; endpoint?: string; user?: string | null };
  master_password_saved?: boolean;
}

// Secret-backend plugin detail. The model: installing this plugin lets it BECOME
// the secret store — a single "use as default" toggle, NOT a picker. When active,
// it owns `codeterm mem secret` and shows unlock/status. Server URL + creds live
// in the plugin's own namespace (config.plugins.<id>).
function SecretBackendDetail({
  plugin,
  config,
}: {
  plugin: InstalledPlugin;
  config: AppConfigShape;
}) {
  const backendId = plugin.id;
  const secretsCfg = config.integrations?.secrets ?? {};
  const [status, setStatus] = useState<SecretStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [orgs, setOrgs] = useState<Array<{ id: string; name: string }>>([]);
  const [colls, setColls] = useState<Array<{ id: string; name: string }>>([]);
  const [defaultOrg, setDefaultOrg] = useState(secretsCfg.default_organization ?? "");
  const [defaultColl, setDefaultColl] = useState(secretsCfg.default_collection ?? "");

  const { base, headers } = apiBaseOf(config);

  // Source of truth is the live status (which backend is actually active), not
  // the config prop — the parent doesn't reload config after a switch, so the
  // prop would go stale and the toggle would lie.
  const configActive = (config.integrations?.secrets?.backend ?? "local") === backendId;
  const isActive = status ? status.backend === backendId : configActive;

  const refresh = useCallback(async () => {
    try {
      const r = await fetch(`${base}/api/secrets/status`, { headers });
      if (r.ok) setStatus((await r.json()) as SecretStatus);
    } catch {
      /* offline — leave status null */
    }
  }, [base, headers]);

  useEffect(() => {
    void refresh();
    // Poll so org/collection appears after the user unlocks inside the iframe.
    const t = setInterval(() => void refresh(), 5000);
    return () => clearInterval(t);
  }, [refresh]);

  // Default org/collection are only enumerable once the vault is unlocked.
  const unlocked = status?.status.status === "unlocked";
  useEffect(() => {
    if (!unlocked) return;
    let cancelled = false;
    (async () => {
      try {
        const [oR, cR] = await Promise.all([
          fetch(`${base}/api/secrets/organizations`, { headers }),
          fetch(`${base}/api/secrets/collections`, { headers }),
        ]);
        if (cancelled) return;
        if (oR.ok) setOrgs((await oR.json()).organizations ?? []);
        if (cR.ok) setColls((await cR.json()).collections ?? []);
      } catch {
        /* leave lists empty */
      }
    })();
    return () => { cancelled = true; };
  }, [unlocked, base, headers]);

  const saveDefaults = useCallback(
    async (org: string, coll: string) => {
      try {
        const cfg = await invoke<AppConfigShape>("get_config");
        const next = {
          ...cfg,
          integrations: {
            ...cfg.integrations,
            secrets: { ...cfg.integrations?.secrets, default_organization: org, default_collection: coll },
          },
        };
        await invoke("update_config", { config: next });
        setError(null);
      } catch (e) {
        setError(`Couldn't save default org/collection: ${String(e)}`);
      }
    },
    [],
  );

  const switchBackend = useCallback(
    async (on: boolean) => {
      setBusy(true);
      setError(null);
      try {
        const r = await fetch(`${base}/api/secrets/backend`, {
          method: "POST",
          headers,
          body: JSON.stringify({ backend: on ? backendId : "local" }),
        });
        if (!r.ok) throw new Error(await readApiError(r, "switch failed"));
        await refresh();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [base, headers, backendId, refresh],
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

  const st = status?.status.status;

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 18 }}>
        <div style={{ width: 40, height: 40, borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(255,255,255,0.04)" }}>
          <PluginIcon plugin={plugin} size={18} />
        </div>
        <div>
          <div style={{ fontSize: 16, fontWeight: 620 }}>{plugin.display_name || plugin.id}</div>
          <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10.5, color: "var(--ct-muted)" }}>
            secret backend{plugin.version ? ` · v${plugin.version}` : ""}
          </div>
        </div>
      </div>

      <label
        style={{
          display: "flex",
          alignItems: "flex-start",
          gap: 10,
          cursor: busy ? "default" : "pointer",
          padding: "12px 14px",
          borderRadius: 8,
          border: "1px solid var(--ct-border-default, rgba(255,255,255,0.1))",
          background: isActive ? "rgba(91,140,255,0.08)" : "transparent",
          marginBottom: 18,
        }}
      >
        <input
          type="checkbox"
          checked={isActive}
          disabled={busy}
          onChange={(e) => void switchBackend(e.target.checked)}
          style={{ marginTop: 2 }}
        />
        <div>
          <div style={{ fontSize: 13, fontWeight: 560 }}>Use as your default secret store</div>
          <div style={{ fontSize: 11.5, color: "var(--ct-muted)", marginTop: 2 }}>
            When on, this plugin handles all <code>codeterm mem secret</code> reads and writes.
            Off falls back to the built-in local file store.
          </div>
        </div>
      </label>

      {isActive && (
        <>
          <GroupLabel>Connection</GroupLabel>
          {/* The plugin's own connection UI (server URL, unlock, status) runs in
              its sandboxed iframe — the host owns only activation + defaults. */}
          <div style={{ height: 360, marginBottom: 18 }}>
            <PluginView pluginId={plugin.id} api={config.api} embedded />
          </div>
          {st === "unlocked" && (
            <>
              <GroupLabel>Default organization</GroupLabel>
              <select
                style={{ ...inputStyle, marginBottom: 14 }}
                value={defaultOrg}
                onChange={(e) => { setDefaultOrg(e.target.value); setDefaultColl(""); void saveDefaults(e.target.value, ""); }}
              >
                <option value="">Personal vault</option>
                {orgs.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}
              </select>
              <GroupLabel>Default collection</GroupLabel>
              <select
                style={inputStyle}
                value={defaultColl}
                onChange={(e) => { setDefaultColl(e.target.value); void saveDefaults(defaultOrg, e.target.value); }}
              >
                <option value="">None</option>
                {colls.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
              <div style={{ fontSize: 11, color: "var(--ct-muted)", marginTop: 4 }}>
                New secrets are filed here by default.
              </div>
            </>
          )}
        </>
      )}

      {error && <div style={{ color: "var(--ct-err, #e57373)", fontSize: 12, marginTop: 4 }}>{error}</div>}
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
                onClick={() => onAction(`install:${d.id}`, { kind: "install", pluginId: d.id, channel: d.channel })}
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

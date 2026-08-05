/**
 * Secret-backend plugin detail (Bitwarden). The model: installing this plugin
 * lets it BECOME the secret store — a single "use as default" toggle, NOT a
 * picker. When active it owns `codeterm mem secret` and shows unlock/status.
 * Server URL + creds live in the plugin's own namespace (config.plugins.<id>),
 * driven by the plugin's sandboxed-iframe UI.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import { PluginIcon } from "./PluginIcon";
import { PluginView } from "./PluginView";
import { apiBaseOf, GroupLabel, iconOf, readApiError, type AppConfigShape } from "./shared";
import type { CatalogEntry } from "./pluginCatalog";
import { ConfigureWithAiButton } from "./ConfigureWithAiButton";

interface SecretStatus {
  backend: string;
  status: { status: "logged_out" | "locked" | "unlocked"; endpoint?: string; user?: string | null };
  master_password_saved?: boolean;
}

export function SecretBackendDetail({
  entry,
  config,
  onAction,
  actionBusy,
}: {
  entry: CatalogEntry;
  config: AppConfigShape;
  onAction?: (kind: "install" | "update" | "remove", entry: CatalogEntry) => void;
  actionBusy?: string | null;
}) {
  const backendId = entry.id;
  const secretsCfg = config.integrations?.secrets ?? {};
  const [status, setStatus] = useState<SecretStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [orgs, setOrgs] = useState<Array<{ id: string; name: string }>>([]);
  const [colls, setColls] = useState<Array<{ id: string; name: string }>>([]);
  const [scopeError, setScopeError] = useState<string | null>(null);
  const [defaultOrg, setDefaultOrg] = useState(secretsCfg.default_organization ?? "");
  const [defaultColl, setDefaultColl] = useState(secretsCfg.default_collection ?? "");

  const { base, headers } = apiBaseOf(config);

  // the config prop — the parent doesn't reload config after a switch.
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
    const t = setInterval(() => void refresh(), 5000);
    return () => clearInterval(t);
  }, [refresh]);

  const unlocked = status?.status.status === "unlocked";
  useEffect(() => {
    if (!unlocked) return;
    let cancelled = false;
    (async () => {
      try {
        const oR = await fetch(`${base}/api/secrets/organizations`, { headers });
        if (cancelled) return;
        if (!oR.ok) throw new Error(await readApiError(oR, "couldn't load organizations"));
        setOrgs((await oR.json()).organizations ?? []);
        setScopeError(null);
      } catch (e) {
        if (!cancelled) setScopeError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [unlocked, base, headers]);

  useEffect(() => {
    if (!unlocked) return;
    let cancelled = false;
    (async () => {
      try {
        const q = defaultOrg ? `?organization=${encodeURIComponent(defaultOrg)}` : "";
        const cR = await fetch(`${base}/api/secrets/collections${q}`, { headers });
        if (cancelled) return;
        if (!cR.ok) throw new Error(await readApiError(cR, "couldn't load collections"));
        setColls((await cR.json()).collections ?? []);
        setScopeError(null);
      } catch (e) {
        if (!cancelled) setScopeError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [unlocked, defaultOrg, base, headers]);

  const saveDefaults = useCallback(async (org: string, coll: string) => {
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
  }, []);

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
    <div data-testid="extensions-secret-backend-detail" data-plugin-id={entry.id}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 18 }}>
        <div style={{ width: 40, height: 40, borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(255,255,255,0.04)" }}>
          <PluginIcon plugin={iconOf(entry)} size={18} />
        </div>
        <div className="min-w-0 flex-1">
          <div style={{ fontSize: 16, fontWeight: 620 }}>{entry.displayName}</div>
          <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10.5, color: "var(--ct-muted)" }}>
            secret backend{entry.version ? ` · v${entry.version}` : ""}
            {entry.updateAvailable && entry.remoteVersion ? ` → v${entry.remoteVersion}` : ""}
          </div>
        </div>
        {entry.updateAvailable && onAction && (
          <button
            data-testid="extensions-plugin-update"
            data-plugin-id={entry.id}
            onClick={() => onAction("update", entry)}
            disabled={actionBusy?.endsWith(`:${entry.id}`) ?? false}
            style={{ font: "inherit", fontSize: 12, fontWeight: 560, padding: "5px 12px", borderRadius: 6, border: "none", cursor: "pointer", whiteSpace: "nowrap", background: "var(--ct-accent, #5b8cff)", color: "#fff" }}
          >
            {actionBusy === `update:${entry.id}` ? "Updating…" : "Update"}
          </button>
        )}
        <ConfigureWithAiButton entry={entry} />
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
          data-testid="extensions-secret-backend-toggle"
          checked={isActive}
          disabled={busy}
          onChange={(e) => void switchBackend(e.target.checked)}
          style={{ marginTop: 2 }}
        />
        <div>
          <div style={{ fontSize: 13, fontWeight: 560 }}>Use as your default secret store</div>
          <div style={{ fontSize: 11.5, color: "var(--ct-muted)", marginTop: 2 }}>
            When on, this plugin handles all <code>codeterm mem secret</code> reads and writes. Off falls back to the
            built-in local file store.
          </div>
        </div>
      </label>

      {isActive && (
        <>
          <GroupLabel>Connection</GroupLabel>
          {/* The plugin's own connection UI (server URL, unlock, status) runs in
              its sandboxed iframe — the host owns only activation + defaults. The
              iframe reports its content height, so this box sizes to fit. */}
          <div style={{ marginBottom: 18 }}>
            <PluginView pluginId={entry.id} api={config.api} embedded />
          </div>
          {st === "unlocked" && (
            <>
              <GroupLabel>Default organization</GroupLabel>
              <select
                data-testid="extensions-secret-default-org"
                style={{ ...inputStyle, marginBottom: 14 }}
                value={defaultOrg}
                onChange={(e) => {
                  setDefaultOrg(e.target.value);
                  setDefaultColl("");
                  void saveDefaults(e.target.value, "");
                }}
              >
                <option value="">Personal vault</option>
                {orgs.map((o) => (
                  <option key={o.id} value={o.id}>
                    {o.name}
                  </option>
                ))}
              </select>
              <GroupLabel>Default collection</GroupLabel>
              <select
                data-testid="extensions-secret-default-collection"
                style={inputStyle}
                value={defaultColl}
                onChange={(e) => {
                  setDefaultColl(e.target.value);
                  void saveDefaults(defaultOrg, e.target.value);
                }}
              >
                <option value="">None</option>
                {colls.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
              <div style={{ fontSize: 11, color: "var(--ct-muted)", marginTop: 4 }}>
                New secrets are filed here by default.
              </div>
              {scopeError && <div style={{ color: "var(--ct-err, #e57373)", fontSize: 12, marginTop: 8 }} data-testid="extensions-secret-scope-error">{scopeError}</div>}
            </>
          )}
        </>
      )}

      {error && <div style={{ color: "var(--ct-err, #e57373)", fontSize: 12, marginTop: 4 }} data-testid="extensions-secret-error">{error}</div>}
    </div>
  );
}

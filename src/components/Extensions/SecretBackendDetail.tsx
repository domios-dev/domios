import { localFetch } from "../../lib/webApi";
import { InlineErrorNotice } from "../shared/ErrorNotice";
/**
 * Secret-backend plugin detail (Bitwarden). The model: installing this plugin
 * lets it BECOME the secret store — a single "use as default" toggle, NOT a
 * picker. When active it owns `codeterm mem secret` and shows unlock/status.
 * Server URL + creds live in the plugin's own namespace (config.plugins.<id>),
 * driven by the plugin's sandboxed-iframe UI.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import { PluginView } from "./PluginView";
import { apiBaseOf, GroupLabel, readApiError, type AppConfigShape } from "./shared";
import type { CatalogEntry } from "./pluginCatalog";
import { PluginDetailHeader, type HeaderActionKind, type HeaderActionOptions } from "./PluginDetailHeader";
import type { PluginActivity } from "../../lib/types";
import type { PluginActivityDisplayMap } from "../../lib/usePluginActivity";

const STATUS_REFRESH_DELAY_MS = 5000;

interface SecretStatus {
  backend: string;
  configured_backend?: string;
  fallback_active?: boolean;
  fallback_reason?: string | null;
  status: { status: "logged_out" | "locked" | "unlocked" | "unavailable"; endpoint?: string; user?: string | null };
  auto_unlock_capable?: boolean;
  master_password_saved?: boolean;
}

export function SecretBackendDetail({
  entry,
  config,
  onAction,
  actionBusy,
  activity,
  activityDisplayMap,
}: {
  entry: CatalogEntry;
  config: AppConfigShape;
  onAction?: (kind: HeaderActionKind, entry: CatalogEntry, options?: HeaderActionOptions) => void;
  actionBusy?: string | null;
  activity?: PluginActivity;
  activityDisplayMap?: PluginActivityDisplayMap;
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
  const isFallbackForThis = Boolean(
    status?.fallback_active &&
      status.configured_backend === backendId &&
      status.backend !== backendId,
  );

  const refresh = useCallback(async () => {
    try {
      const r = await localFetch(`/api/secrets/status`, { headers });
      if (r.ok) setStatus((await r.json()) as SecretStatus);
    } catch {
      /* offline — leave status null */
    }
  }, [base, headers]);

  // Chains the next refresh off THIS one's completion instead of a fixed interval, so a backend slower than the delay self-throttles rather than queueing overlapping requests.
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const tick = () => {
      void refresh().finally(() => {
        if (!cancelled) timer = setTimeout(tick, STATUS_REFRESH_DELAY_MS);
      });
    };
    tick();

    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [refresh]);

  const unlocked = status?.status.status === "unlocked";
  useEffect(() => {
    if (!unlocked) return;
    let cancelled = false;
    (async () => {
      try {
        const oR = await localFetch(`/api/secrets/organizations`, { headers });
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
  }, [unlocked, base, headers.Authorization]);

  useEffect(() => {
    if (!unlocked) return;
    let cancelled = false;
    (async () => {
      try {
        const q = defaultOrg ? `?organization=${encodeURIComponent(defaultOrg)}` : "";
        const cR = await localFetch(`/api/secrets/collections${q}`, { headers });
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
  }, [unlocked, defaultOrg, base, headers.Authorization]);

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
        const r = await localFetch(`/api/secrets/backend`, {
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

  const st = status?.status.status;

  return (
    <div className="flex flex-col gap-4" data-testid="extensions-secret-backend-detail" data-plugin-id={entry.id}>
      <PluginDetailHeader
        entry={entry}
        busy={actionBusy}
        onAction={(kind, target, options) => onAction?.(kind, target, options)}
        activity={activity}
        activityDisplayMap={activityDisplayMap}
      />

      <label
        className="ct-card ct-card--static flex items-start gap-2.5 px-3.5 py-3"
        style={{
          cursor: busy ? "default" : "pointer",
          background: isActive ? "color-mix(in srgb, var(--ct-accent) 8%, var(--ct-bg))" : undefined,
        }}
      >
        <input
          type="checkbox"
          data-testid="extensions-secret-backend-toggle"
          checked={isActive}
          disabled={busy}
          onChange={(e) => void switchBackend(e.target.checked)}
          className="mt-0.5"
        />
        <div>
          <div className="text-[13px] font-medium text-[var(--ct-fg)]">Use as your default secret store</div>
          <div className="mt-0.5 text-xs leading-relaxed text-[var(--ct-muted-fg)]">
            When on, this plugin handles all <code>codeterm mem secret</code> reads and writes. Off falls back to the
            built-in local file store.
          </div>
        </div>
      </label>

      {isFallbackForThis && (
        <div
          role="status"
          data-testid="extensions-secret-backend-fallback"
          className="ct-card ct-card--static border border-[var(--ct-yellow)]/40 px-3.5 py-3 text-xs space-y-1"
        >
          <div className="font-medium text-[var(--ct-yellow)]">
            Local secret storage is active ({backendId} unavailable)
          </div>
          <div className="text-[var(--ct-muted-fg)] leading-relaxed">
            {status?.fallback_reason ||
              `This plugin is configured as the default secret store, but local storage is active until a compatible version loads.`}
          </div>
        </div>
      )}

      {isActive && (
        <>
          <div>
            <PluginView pluginId={entry.id} api={config.api} embedded />
          </div>
          {st === "unlocked" && (
            <div className="ct-card ct-card--static space-y-3 p-4">
              <div>
                <GroupLabel className="mb-1.5 text-xs">Default organization</GroupLabel>
                <select
                  data-testid="extensions-secret-default-org"
                  className="ct-input ct-input--compact w-full"
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
              </div>
              <div>
                <GroupLabel className="mb-1.5 text-xs">Default collection</GroupLabel>
                <select
                  data-testid="extensions-secret-default-collection"
                  className="ct-input ct-input--compact w-full"
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
                <div className="mt-1 text-[11px] text-[var(--ct-muted-fg)]">
                  New secrets are filed here by default.
                </div>
              </div>
              {scopeError && <div data-testid="extensions-secret-scope-error"><InlineErrorNotice error={scopeError} operation="updating the secret store" /></div>}
            </div>
          )}
        </>
      )}

      {error && <div data-testid="extensions-secret-error"><InlineErrorNotice error={error} operation="updating the secret store" /></div>}
    </div>
  );
}

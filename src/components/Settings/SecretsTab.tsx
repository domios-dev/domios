import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "../../lib/bridge";
import { KeyRound, LogIn, FlaskConical, Settings2, Clock } from "lucide-react";
import type { AppConfig, SecretsConfig } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { setPendingCommand } from "../../lib/pendingCommands";
import { SettingsCard, SettingsPill } from "./shared";

type SecretsBackend = "local" | "bitwarden" | "mock";

const BACKEND_LABELS: Record<SecretsBackend, string> = {
  local: "Local secret storage",
  bitwarden: "Bitwarden",
  mock: "Mock (testing only)",
};

interface SecretsStatus {
  backend: string;
  status: {
    status: "logged_out" | "locked" | "unlocked" | "unavailable";
    user?: string | null;
    transient?: boolean;
    endpoint?: string | null;
    reason?: string;
  };
  master_password_saved?: boolean;
}

interface SecretListItem {
  item_id: string;
  name: string;
  folder?: string | null;
  organization?: string | null;
  collection?: string | null;
  last_modified?: number | null;
}

export function SecretsTab({
  config,
  onUpdate,
  onCloseSettings,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onCloseSettings?: () => void;
}) {
  const api = config.api;
  const apiBase = api ? `http://${api.host === "0.0.0.0" ? "127.0.0.1" : api.host}:${api.port}` : "";
  const apiHeaders: HeadersInit = api?.token
    ? { Authorization: `Bearer ${api.token}` }
    : {};
  const jsonHeaders: HeadersInit = { "Content-Type": "application/json", ...apiHeaders };

  const { state: muxState } = useMultiplexer();
  const installBitwardenCli = useCallback(async () => {
    const platform = navigator.platform.toLowerCase();
    const cmd = platform.includes("mac")
      ? "brew install bitwarden-cli"
      : platform.includes("win")
        ? "scoop install bitwarden-cli"
        : "npm i -g @bitwarden/cli";

    const ws =
      muxState.workspaces.find((w) => w.id === muxState.active_workspace_id) ||
      muxState.workspaces[0];
    if (!ws) return;
    try {
      const tabInfo = await invoke<{ id: string; title: string; pane_id: string }>(
        "mux_create_tab",
        { workspaceId: ws.id, activate: true },
      );
      setPendingCommand(tabInfo.pane_id, cmd);
      await invoke("mux_set_active_workspace", { workspaceId: ws.id });
      await invoke("mux_set_active_tab", { workspaceId: ws.id, tabId: tabInfo.id });
      // Tell the host UI to switch back to the workspace view (away from
      // Settings) so the user actually sees the install running.
      onCloseSettings?.();
    } catch (e) {
      console.error("Failed to spawn install tab:", e);
    }
  }, [muxState, onCloseSettings]);

  const cfg = config.integrations?.secrets ?? {
    backend: "local",
    default_organization: "",
    default_collection: "",
    bitwarden: { server_url: "https://vault.bitwarden.com" },
  };
  const backend = (cfg.backend || "local") as SecretsBackend;
  const bitwardenServerUrl =
    (cfg as { bitwarden?: { server_url?: string } }).bitwarden?.server_url ||
    "https://vault.bitwarden.com";

  const [status, setStatus] = useState<SecretsStatus | null>(null);
  const [statusBusy, setStatusBusy] = useState(false);
  const [items, setItems] = useState<SecretListItem[]>([]);
  const [orgs, setOrgs] = useState<{ id: string; name: string }[]>([]);
  const [colls, setColls] = useState<{ id: string; name: string }[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"password" | "apikey">("password");
  const [toast, setToast] = useState<string | null>(null);
  const [serverUrlInput, setServerUrlInput] = useState(bitwardenServerUrl);

  // Login form state — never persisted in component state past submission.
  const [email, setEmail] = useState("");
  const [masterPassword, setMasterPassword] = useState("");
  const [twoFactor, setTwoFactor] = useState("");
  const [apiClientId, setApiClientId] = useState("");
  const [apiClientSecret, setApiClientSecret] = useState("");

  const refreshStatus = useCallback(async () => {
    if (!apiBase) return;
    setStatusBusy(true);
    try {
      const r = await fetch(`${apiBase}/api/secrets/status`, { headers: apiHeaders });
      if (r.ok) setStatus(await r.json());
    } catch (e) {
      console.warn("secrets status failed:", e);
    } finally {
      setStatusBusy(false);
    }
  }, [apiBase]);

  const refreshItems = useCallback(async () => {
    if (!apiBase) return;
    try {
      const r = await fetch(`${apiBase}/api/secrets/items?limit=10`, { headers: apiHeaders });
      if (r.ok) {
        const d = await r.json();
        setItems(d.items ?? []);
      } else {
        setItems([]);
      }
    } catch {
      setItems([]);
    }
  }, [apiBase]);

  const refreshOrgs = useCallback(async () => {
    if (!apiBase) return;
    try {
      const r = await fetch(`${apiBase}/api/secrets/organizations`, { headers: apiHeaders });
      if (r.ok) {
        const d = await r.json();
        setOrgs(d.organizations ?? []);
      }
    } catch {
      // ignore
    }
  }, [apiBase]);

  const refreshColls = useCallback(
    async (orgId: string) => {
      if (!apiBase) return;
      const url = orgId
        ? `${apiBase}/api/secrets/collections?organization=${encodeURIComponent(orgId)}`
        : `${apiBase}/api/secrets/collections`;
      try {
        const r = await fetch(url, { headers: apiHeaders });
        if (r.ok) {
          const d = await r.json();
          setColls(d.collections ?? []);
        }
      } catch {
        // ignore
      }
    },
    [apiBase],
  );

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus, backend]);

  useEffect(() => {
    setServerUrlInput(bitwardenServerUrl);
  }, [bitwardenServerUrl]);

  useEffect(() => {
    if (status?.status?.status === "unlocked") {
      refreshItems();
      refreshOrgs();
      refreshColls(cfg.default_organization);
    } else {
      setItems([]);
      setOrgs([]);
      setColls([]);
    }
  }, [status?.status?.status, refreshItems, refreshOrgs, refreshColls, cfg.default_organization]);

  // Single seam for "merge a partial change into integrations.secrets
  // without clobbering siblings". Every settings handler funnels through
  // here so we cannot accidentally drop a field (we hit that bug twice).
  const patchSecrets = useCallback(
    (patch: Partial<SecretsConfig>) => {
      const merged: SecretsConfig = {
        backend: cfg.backend,
        default_organization: cfg.default_organization,
        default_collection: cfg.default_collection,
        bitwarden: cfg.bitwarden ?? { server_url: bitwardenServerUrl, persist_master_password: false },
        ...patch,
      };
      onUpdate("integrations", { secrets: merged });
    },
    [
      onUpdate,
      cfg.backend,
      cfg.default_organization,
      cfg.default_collection,
      cfg.bitwarden,
      bitwardenServerUrl,
    ],
  );

  const setBackend = useCallback(
    async (next: SecretsBackend) => {
      setError(null);
      patchSecrets({ backend: next });
      // Drop stale UI state from the previous backend before the swap.
      setItems([]);
      setOrgs([]);
      setColls([]);
      setStatus(null);
      try {
        const r = await fetch(`${apiBase}/api/secrets/backend`, {
          method: "POST",
          headers: jsonHeaders,
          body: JSON.stringify({
            backend: next,
            bitwarden_server_url: next === "bitwarden" ? bitwardenServerUrl : undefined,
          }),
        });
        if (!r.ok) {
          const d = await r.json().catch(() => ({}));
          setError(d?.error?.message || `switch failed (HTTP ${r.status})`);
          return;
        }
        const d = await r.json();
        setStatus({ backend: d.backend, status: d.status });
        setToast(`Switched to ${BACKEND_LABELS[next]}`);
        setTimeout(() => setToast(null), 2500);
      } catch (e) {
        setError(String(e));
      }
    },
    [apiBase, jsonHeaders, patchSecrets, bitwardenServerUrl],
  );

  const saveServerUrl = useCallback(async () => {
    setError(null);
    const url = serverUrlInput.trim();
    if (!/^https?:\/\/[^\s/$.?#].[^\s]*$/i.test(url)) {
      setError("Server URL must start with http:// or https://");
      return;
    }
    patchSecrets({
      bitwarden: { ...(cfg.bitwarden ?? { persist_master_password: false }), server_url: url },
    });
    try {
      const r = await fetch(`${apiBase}/api/secrets/backend`, {
        method: "POST",
        headers: jsonHeaders,
        body: JSON.stringify({ backend: "bitwarden", bitwarden_server_url: url }),
      });
      if (!r.ok) {
        const d = await r.json().catch(() => ({}));
        setError(d?.error?.message || `apply failed (HTTP ${r.status})`);
        return;
      }
      const d = await r.json();
      setStatus({ backend: d.backend, status: d.status });
      setToast(`Server URL applied — sign in again`);
      setTimeout(() => setToast(null), 2500);
    } catch (e) {
      setError(String(e));
    }
  }, [apiBase, jsonHeaders, patchSecrets, cfg.bitwarden, serverUrlInput]);

  const setDefaultOrg = useCallback(
    (orgId: string) => {
      patchSecrets({ default_organization: orgId, default_collection: "" });
    },
    [patchSecrets],
  );

  const setDefaultColl = useCallback(
    (collId: string) => {
      patchSecrets({ default_collection: collId });
    },
    [patchSecrets],
  );

  const submitPasswordLogin = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      setError(null);
      try {
        const r = await fetch(`${apiBase}/api/secrets/unlock`, {
          method: "POST",
          headers: jsonHeaders,
          body: JSON.stringify({
            master_password: masterPassword,
            email: email || undefined,
            two_factor_token: twoFactor || undefined,
          }),
        });
        if (!r.ok) {
          const d = await r.json().catch(() => ({}));
          setError(d?.error?.message || `unlock failed (HTTP ${r.status})`);
          return;
        }
        // Wipe form state — password lifetime ends here.
        setMasterPassword("");
        setTwoFactor("");
        await refreshStatus();
      } catch (err) {
        setError(String(err));
      }
    },
    [apiBase, jsonHeaders, masterPassword, email, twoFactor, refreshStatus],
  );

  const submitApiKeyLogin = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      setError(null);
      try {
        const r = await fetch(`${apiBase}/api/secrets/unlock`, {
          method: "POST",
          headers: jsonHeaders,
          body: JSON.stringify({
            master_password: masterPassword,
            api_key_client_id: apiClientId,
            api_key_client_secret: apiClientSecret,
          }),
        });
        if (!r.ok) {
          const d = await r.json().catch(() => ({}));
          setError(d?.error?.message || `unlock failed (HTTP ${r.status})`);
          return;
        }
        setMasterPassword("");
        setApiClientSecret("");
        await refreshStatus();
      } catch (err) {
        setError(String(err));
      }
    },
    [apiBase, jsonHeaders, masterPassword, apiClientId, apiClientSecret, refreshStatus],
  );

  const logout = useCallback(async () => {
    if (!confirm("Sign out? This deletes the saved master password and session token.")) return;
    setError(null);
    await fetch(`${apiBase}/api/secrets/logout`, { method: "POST", headers: jsonHeaders, body: "{}" });
    await refreshStatus();
  }, [apiBase, jsonHeaders, refreshStatus]);

  const statusBadge = useMemo(() => {
    if (!status) return null;
    const s = status.status;
    if (s.status === "unavailable") return <SettingsPill tone="red">unavailable</SettingsPill>;
    if (s.status === "logged_out") return <SettingsPill tone="muted">logged out</SettingsPill>;
    // Locked + Unlocked are both "signed in" — daemon JIT-unlocks transparently.
    return (
      <SettingsPill tone="green">
        logged in{s.user ? ` as ${s.user}` : ""}
      </SettingsPill>
    );
  }, [status]);

  const inputClass =
    "w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none";

  return (
    <div className="flex flex-col gap-5">
      <SettingsCard
        icon={<KeyRound size={16} />}
        title="Secret store"
        headerRight={
          <div className="flex items-center gap-2">
            {statusBadge}
            <button
              onClick={refreshStatus}
              disabled={statusBusy}
              className="text-[10px] px-2 py-0.5 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] disabled:opacity-50"
            >
              refresh
            </button>
          </div>
        }
        description={
          <>
            Pluggable backend for the <code className="font-mono">codeterm mem secret</code> command family.
            Secrets do not cross the mesh — only the local machine's vault is reachable.
          </>
        }
      >
        <div className="space-y-1">
          <div className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">Backend</div>
          <select
            value={backend}
            onChange={(e) => void setBackend(e.target.value as SecretsBackend)}
            className={inputClass}
          >
            <option value="local">Local secret storage</option>
            <option value="bitwarden">Bitwarden</option>
            <option value="mock">Mock (testing only)</option>
          </select>
          <div className="text-[10px] text-[var(--ct-muted-fg)]">
            {backend === "local"
              ? "Persisted in the OS keychain (macOS Keychain / libsecret / Windows Credential Manager). Offline, no external service."
              : backend === "bitwarden"
                ? "Vault accessed via the bw CLI. Supports vault.bitwarden.com and self-hosted Vaultwarden."
                : "In-memory only. Data is wiped on daemon restart — for tests and local dev."}
          </div>
        </div>

        {backend === "bitwarden" && (
          <div className="space-y-1">
            <div className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">
              Server URL
            </div>
            <div className="flex gap-2">
              <input
                type="text"
                value={serverUrlInput}
                onChange={(e) => setServerUrlInput(e.target.value)}
                placeholder="https://vault.bitwarden.com"
                className={inputClass}
                spellCheck={false}
              />
              <button
                onClick={() => void saveServerUrl()}
                disabled={serverUrlInput.trim() === bitwardenServerUrl}
                className="text-xs px-3 py-1.5 rounded border border-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Apply
              </button>
            </div>
            <div className="text-[10px] text-[var(--ct-muted-fg)]">
              Self-hosted Vaultwarden? Point to{" "}
              <code className="font-mono">https://your-host.example</code>. Changing the URL forces a re-login.
            </div>
            {backend === "bitwarden" &&
              (status?.status?.status === "unlocked" || status?.status?.status === "locked") && (
                status?.master_password_saved ? (
                  <div className="text-[11px] text-[var(--ct-green)] pt-1">
                    ✓ Master password saved — daemon auto-unlocks on every operation.
                  </div>
                ) : (
                  <div className="pt-1 space-y-1">
                    <div className="text-[11px] text-[var(--ct-amber)]">
                      ⚠ Master password not saved — daemon will lock and prompt on restart.
                    </div>
                    <form onSubmit={submitPasswordLogin} className="flex gap-2">
                      <input
                        type="password"
                        placeholder="master password"
                        autoComplete="current-password"
                        value={masterPassword}
                        onChange={(e) => setMasterPassword(e.target.value)}
                        className={inputClass}
                        required
                      />
                      <button
                        type="submit"
                        className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90 whitespace-nowrap"
                      >
                        Save
                      </button>
                    </form>
                    <div className="text-[10px] text-[var(--ct-muted-fg)]">
                      Stored as 0600 file in <code className="font-mono">~/.codeterm/secrets/bitwarden-master.env</code>. Copy to mesh peers for fleet-wide auto-unlock.
                    </div>
                  </div>
                )
              )}
          </div>
        )}

        {toast && (
          <div className="text-[11px] rounded border border-[var(--ct-green)]/40 bg-[var(--ct-green)]/10 p-2 text-[var(--ct-green)]">
            {toast}
          </div>
        )}

        {backend === "bitwarden" && status?.status?.status === "locked" && (
          <div className="flex items-center gap-2 pt-1">
            <button
              onClick={logout}
              className="text-[11px] px-3 py-1.5 rounded border border-[var(--ct-red)]/40 text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10"
            >
              Sign out
            </button>
            <span className="text-[10px] text-[var(--ct-muted-fg)]">
              Clears saved master password and session token.
            </span>
          </div>
        )}

        {status?.status?.status === "unavailable" && (
          <div className="text-[11px] rounded border border-[var(--ct-red)]/40 bg-[var(--ct-red)]/10 p-3 text-[var(--ct-red)] space-y-2">
            <div>
              <div className="font-medium mb-1">Backend unavailable</div>
              <div className="text-[var(--ct-fg)]">{status.status.reason}</div>
            </div>
            {backend === "bitwarden" && (
              <div className="flex items-center gap-2 pt-1">
                <button
                  onClick={() => void installBitwardenCli()}
                  className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90 font-medium"
                >
                  Install Bitwarden CLI
                </button>
                <span className="text-[10px] text-[var(--ct-muted-fg)]">
                  Opens a new tab and runs the right install command for your platform.
                </span>
              </div>
            )}
          </div>
        )}

        {error && (
          <div className="text-[11px] rounded border border-[var(--ct-red)]/40 bg-[var(--ct-red)]/10 p-2 text-[var(--ct-red)]">
            {error}
          </div>
        )}
      </SettingsCard>

      {status?.status?.status === "logged_out" && backend === "bitwarden" && (
        <SettingsCard icon={<LogIn size={16} />} title="Sign in to Bitwarden">
          <div className="flex gap-1 text-[11px]">
            <button
              onClick={() => setActiveTab("password")}
              className={`px-2.5 py-1 rounded border ${activeTab === "password" ? "bg-[var(--ct-accent)] text-[var(--ct-bg)] border-[var(--ct-accent)]" : "bg-transparent text-[var(--ct-muted-fg)] border-[var(--ct-border)]"}`}
            >
              Password
            </button>
            <button
              onClick={() => setActiveTab("apikey")}
              className={`px-2.5 py-1 rounded border ${activeTab === "apikey" ? "bg-[var(--ct-accent)] text-[var(--ct-bg)] border-[var(--ct-accent)]" : "bg-transparent text-[var(--ct-muted-fg)] border-[var(--ct-border)]"}`}
            >
              API key
            </button>
          </div>
          {activeTab === "password" ? (
            <form onSubmit={submitPasswordLogin} className="space-y-2">
              <input
                type="email"
                placeholder="email"
                autoComplete="username"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className={inputClass}
                required
              />
              <input
                type="password"
                placeholder="master password (never persisted)"
                autoComplete="current-password"
                value={masterPassword}
                onChange={(e) => setMasterPassword(e.target.value)}
                className={inputClass}
                required
              />
              <input
                type="text"
                placeholder="2FA code (optional)"
                value={twoFactor}
                onChange={(e) => setTwoFactor(e.target.value)}
                className={inputClass}
              />
              <button
                type="submit"
                className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90"
              >
                Sign in
              </button>
            </form>
          ) : (
            <form onSubmit={submitApiKeyLogin} className="space-y-2">
              <div className="text-[10px] text-[var(--ct-muted-fg)]">
                Create a personal API key at vault.bitwarden.com → Security → Keys.
              </div>
              <input
                type="text"
                placeholder="client_id"
                value={apiClientId}
                onChange={(e) => setApiClientId(e.target.value)}
                className={inputClass}
                required
              />
              <input
                type="password"
                placeholder="client_secret"
                value={apiClientSecret}
                onChange={(e) => setApiClientSecret(e.target.value)}
                className={inputClass}
                required
              />
              <input
                type="password"
                placeholder="master password (still required to unlock)"
                autoComplete="current-password"
                value={masterPassword}
                onChange={(e) => setMasterPassword(e.target.value)}
                className={inputClass}
                required
              />
              <button
                type="submit"
                className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90"
              >
                Sign in with API key
              </button>
            </form>
          )}
        </SettingsCard>
      )}

      {backend === "mock" && status?.status?.status !== "unlocked" && (
        <SettingsCard
          icon={<FlaskConical size={16} />}
          title="Mock backend (testing only)"
          description={
            <>
              In-memory store, no auth. Anything you "save" lives until the daemon restarts. Use{" "}
              <strong>Local secret storage</strong> for real secrets.
            </>
          }
        >
          <button
            onClick={() => {
              setMasterPassword("dev-only");
              submitPasswordLogin(new Event("submit") as unknown as React.FormEvent);
            }}
            className="text-xs px-3 py-1.5 rounded bg-[var(--ct-accent)] text-[var(--ct-bg)] hover:opacity-90"
          >
            Activate mock store
          </button>
        </SettingsCard>
      )}

      {status?.status?.status === "unlocked" && (
        <>
          <SettingsCard
            icon={<Settings2 size={16} />}
            title={backend === "bitwarden" ? "Default scope" : "Connected"}
            headerRight={
              status.status.endpoint ? (
                <span className="text-[10px] font-mono text-[var(--ct-muted-fg)]">
                  {status.status.endpoint}
                </span>
              ) : undefined
            }
            description={
              backend === "bitwarden" ? (
                <>
                  Where new secrets land when no <code className="font-mono">--org</code> /{" "}
                  <code className="font-mono">--collection</code> is given.
                </>
              ) : undefined
            }
          >
            {backend === "bitwarden" && (
              <>
                <div className="space-y-1">
                  <div className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">Organization</div>
                  <select
                    value={cfg.default_organization}
                    onChange={(e) => setDefaultOrg(e.target.value)}
                    className={inputClass}
                  >
                    <option value="">Personal vault</option>
                    {orgs.map((o) => (
                      <option key={o.id} value={o.id}>
                        {o.name}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="space-y-1">
                  <div className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">Collection</div>
                  <select
                    value={cfg.default_collection}
                    onChange={(e) => setDefaultColl(e.target.value)}
                    className={inputClass}
                    disabled={!colls.length}
                  >
                    <option value="">(none)</option>
                    {colls.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.name}
                      </option>
                    ))}
                  </select>
                </div>
              </>
            )}
            <div className="flex gap-2 pt-1">
              <button
                onClick={logout}
                className="text-xs px-3 py-1.5 rounded border border-[var(--ct-red)]/40 text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10"
              >
                Sign out
              </button>
            </div>
          </SettingsCard>

          <SettingsCard
            icon={<Clock size={16} />}
            title="Recently saved"
            headerRight={
              <button
                onClick={refreshItems}
                className="text-[10px] px-2 py-0.5 rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
              >
                refresh
              </button>
            }
          >
            {items.length === 0 ? (
              <div className="text-[11px] text-[var(--ct-muted-fg)]">
                No secrets saved yet. Use{" "}
                <code className="font-mono text-[var(--ct-fg)]">codeterm mem secret save --name X</code>{" "}
                from any pane.
              </div>
            ) : (
              <div className="space-y-1">
                {items.map((it) => (
                  <div key={it.item_id} className="flex items-center justify-between text-[11px] font-mono">
                    <span className="text-[var(--ct-fg)] truncate">{it.name}</span>
                    <span className="text-[var(--ct-muted)] truncate ml-3">
                      {it.folder ? `folder:${it.folder.slice(0, 8)} ` : ""}
                      {it.organization ? `org:${it.organization.slice(0, 8)}` : ""}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </SettingsCard>
        </>
      )}
    </div>
  );
}

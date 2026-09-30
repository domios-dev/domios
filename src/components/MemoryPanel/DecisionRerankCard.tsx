import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { ExternalLink, Sparkles } from "lucide-react";
import type { AppConfig, DecisionRerankConfig, MemoryConfig } from "../../lib/types";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { AboutLink, SettingsCard, Toggle } from "../Settings/shared";

const DEFAULT_ADAPTER = "typesafe-jev";
const DOCS_URL = "https://docs.typesafe.ai";
const KEYS_URL = "https://console.typesafe.ai/keys";

interface AdapterTokenStatus {
  present: boolean;
  mode_ok: boolean;
}

interface DecisionStatus {
  adapter_registered: boolean;
  adapters: { id: string; token: AdapterTokenStatus }[];
}

interface Props {
  mem: MemoryConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  apiBase: string;
  apiHeaders: HeadersInit;
}

export function DecisionRerankCard({ mem, onUpdate, apiBase, apiHeaders }: Props) {
  const adapter = mem.decision_rerank?.adapter ?? DEFAULT_ADAPTER;
  const inputId = useId();
  const tokenRef = useRef<HTMLInputElement>(null);
  const [status, setStatus] = useState<DecisionStatus | null>(null);
  const [statusFailed, setStatusFailed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const authRef = useRef(apiHeaders);
  authRef.current = apiHeaders;

  const refreshStatus = useCallback(async () => {
    try {
      const r = await fetch(`${apiBase}/api/mem/decision/status`, { headers: authRef.current });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      setStatus(await r.json() as DecisionStatus);
      setStatusFailed(false);
    } catch {
      setStatusFailed(true);
    }
  }, [apiBase]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const clearInput = () => {
    if (tokenRef.current) tokenRef.current.value = "";
  };

  const saveToken = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const token = (tokenRef.current?.value ?? "").trim();
    if (!token || busy) return;
    setBusy(true);
    setError(null);
    try {
      const r = await fetch(`${apiBase}/api/mem/decision/token`, {
        method: "POST",
        headers: { "Content-Type": "application/json", ...authRef.current },
        body: JSON.stringify({ adapter, token }),
      });
      if (!r.ok) setError(`Could not save the token (HTTP ${r.status}).`);
    } catch {
      setError("Could not reach the daemon to save the token.");
    } finally {
      clearInput();
    }
    await refreshStatus();
    setBusy(false);
  };

  const clearToken = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const r = await fetch(`${apiBase}/api/mem/decision/token?adapter=${encodeURIComponent(adapter)}`, {
        method: "DELETE",
        headers: authRef.current,
      });
      if (!r.ok) setError(`Could not clear the token (HTTP ${r.status}).`);
    } catch {
      setError("Could not reach the daemon to clear the token.");
    } finally {
      clearInput();
    }
    await refreshStatus();
    setBusy(false);
  };

  const token = status?.adapters?.find((a) => a.id === adapter)?.token;
  const tokenState = token ? (token.present ? "set" : "not set") : null;

  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Decision re-rank"
      description="A fast decision model re-ranks your memory recall to surface the most relevant results and flag when nothing is actually relevant."
    >
      <Toggle
        label="Re-rank memory recall"
        checked={mem.decision_rerank?.enabled ?? false}
        onChange={(v) =>
          onUpdate("memory", {
            ...mem,
            decision_rerank: { ...(mem.decision_rerank ?? {}), enabled: v } as DecisionRerankConfig,
          })
        }
      />

      <form onSubmit={saveToken} className="flex flex-col gap-2" data-testid="decision-rerank-token-form">
        <FormRow label="API token" htmlFor={inputId} hint="Write-only. It is sent to the local daemon and never shown again.">
          <input
            id={inputId}
            ref={tokenRef}
            type="password"
            autoComplete="off"
            spellCheck={false}
            defaultValue=""
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
            data-testid="decision-rerank-token-input"
          />
        </FormRow>
        <div className="flex items-center gap-2">
          <Button type="submit" variant="accent" size="sm" disabled={busy} data-testid="decision-rerank-token-save">
            Save token
          </Button>
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void clearToken()} data-testid="decision-rerank-token-clear">
            Clear token
          </Button>
        </div>
      </form>

      <div className="flex flex-col gap-1 text-[11px] text-[var(--ct-muted-fg)]" role="status" data-testid="decision-rerank-status">
        {tokenState && (
          <p>
            Token for {adapter}: <span data-testid="decision-rerank-token-state">{tokenState}</span>
          </p>
        )}
        {statusFailed && <p data-testid="decision-rerank-status-failed">Token status is unavailable.</p>}
        {token && !token.mode_ok && (
          <p data-testid="decision-rerank-mode-warning">Warning: the token file permissions are not owner-only.</p>
        )}
        {status && !status.adapter_registered && (
          <p data-testid="decision-rerank-adapter-missing">The adapter {adapter} is not registered in this build.</p>
        )}
        {error && <p role="alert" data-testid="decision-rerank-error">{error}</p>}
      </div>

      <div className="flex flex-wrap items-center gap-4" data-testid="decision-rerank-help">
        <AboutLink href={DOCS_URL} label="TypeSafe docs" Icon={ExternalLink} />
        <AboutLink href={KEYS_URL} label="Get a token" Icon={ExternalLink} />
      </div>
    </SettingsCard>
  );
}

import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";

export interface SupporterTokenStatus {
  adapter: string;
  token_mode: "required" | "optional" | "none";
  present: boolean;
  path?: string;
  mode_ok?: boolean | null;
}

export interface SupporterAdapter {
  id: string;
  token_mode: SupporterTokenStatus["token_mode"];
  primitives: { choice: boolean; noul: boolean; score: boolean };
  models: { id: string; display_name: string }[];
  token: SupporterTokenStatus;
}

export interface SupporterSpendTotals {
  input_tokens: number;
  output_tokens: number;
  estimated_usd: number | null;
}

export interface SupporterSpend {
  today: SupporterSpendTotals;
  month: SupporterSpendTotals;
  since_balance: SupporterSpendTotals;
  balance_usd: number | null;
  balance_set_at: number | null;
  remaining_usd: number | null;
  exhausted: boolean;
  next_probe_at: number | null;
  storage_ready: boolean;
}

export interface SupporterStatus {
  state:
    | "active"
    | "credit_exhausted"
    | "budget_reached"
    | "not_connected"
    | "no_token"
    | "token_unusable"
    | "adapter_unavailable";
  token_mode: SupporterTokenStatus["token_mode"];
  adapter: string;
  model: string | null;
  deadline_ms: number;
  connected: boolean;
  last_activity: number | null;
  calls_last_hour: number;
  calls_last_day: number;
  last_verdict: Partial<
    Record<"rerank" | "tab_state", { time: number; label: string | null; confidence: number | null }>
  >;
  budget: {
    reached: boolean;
    calls_per_hour: number | null;
    free_at: number | null;
    storage_ready: boolean;
  };
  pricing?: { display_name: string; input_usd_per_million: number; output_usd_per_million: number } | null;
  spend: SupporterSpend;
  token: SupporterTokenStatus;
  adapter_registered: boolean;
  adapters: SupporterAdapter[];
}

export function useDecisionToken(apiBase: string, apiHeaders: HeadersInit) {
  const tokenRef = useRef<HTMLInputElement>(null);
  const [status, setStatus] = useState<SupporterStatus | null>(null);
  const [statusFailed, setStatusFailed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const authRef = useRef(apiHeaders);
  authRef.current = apiHeaders;

  const refreshStatus = useCallback(async () => {
    try {
      const response = await fetch(`${apiBase}/api/supporter/status`, {
        headers: authRef.current,
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setStatus((await response.json()) as SupporterStatus);
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

  const write = async (url: string, init: RequestInit, verb: string, secret?: string): Promise<boolean> => {
    setBusy(true);
    setError(null);
    let ok = false;
    try {
      const response = await fetch(url, {
        ...init,
        headers: { ...authRef.current, ...init.headers },
      });
      if (!response.ok) {
        const body = (await response.json().catch(() => null)) as {
          error?: { message?: string };
        } | null;
        const message = body?.error?.message?.replace(secret ?? "\u0000", "[redacted]");
        setError(message || `Could not ${verb} (HTTP ${response.status}).`);
      } else {
        ok = true;
      }
    } catch {
      setError(`Could not reach the daemon to ${verb}.`);
    } finally {
      clearInput();
    }
    await refreshStatus();
    setBusy(false);
    return ok;
  };

  const saveToken = async (event: FormEvent<HTMLFormElement>): Promise<boolean> => {
    event.preventDefault();
    const token = (tokenRef.current?.value ?? "").trim();
    if (!token || busy) return false;
    return write(
      `${apiBase}/api/supporter/token`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ adapter: status?.adapter, token }),
      },
      "save the token",
      token,
    );
  };

  const clearToken = async () => {
    if (busy || !status) return;
    await write(
      `${apiBase}/api/supporter/token?adapter=${encodeURIComponent(status.adapter)}`,
      { method: "DELETE" },
      "clear the token",
    );
  };

  const select = async (adapter: string, model?: string) => {
    if (busy) return;
    await write(
      `${apiBase}/api/supporter/use`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ adapter, model }),
      },
      "select the Supporter Agent model",
    );
  };

  const setBalance = async (balance: number | null) => {
    if (busy || !status) return;
    await write(
      `${apiBase}/api/supporter/balance`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ adapter: status.adapter, balance_usd: balance }),
      },
      "save the balance",
    );
  };

  return {
    tokenRef,
    status,
    busy,
    error,
    statusFailed,
    tokenState: status?.token.present ? "set" : status ? "not set" : null,
    modeWarning: status?.token.present === true && status.token.mode_ok === false,
    adapterMissing: status !== null && !status.adapter_registered,
    refreshStatus,
    saveToken,
    clearToken,
    select,
    setBalance,
  };
}

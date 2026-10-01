import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";

export interface SupporterTokenStatus {
  adapter: string;
  present: boolean;
  path: string;
  mode_ok: boolean | null;
}

export interface SupporterAdapter {
  id: string;
  primitives: { choice: boolean; noul: boolean; score: boolean };
  models: { id: string; display_name: string }[];
  token: SupporterTokenStatus;
}

export interface SupporterStatus {
  state: "active" | "token_only" | "no_token" | "feature_without_token" | "token_unusable" | "adapter_unavailable";
  adapter: string;
  model: string | null;
  deadline_ms: number;
  features: { memory_rerank: boolean; tab_state: boolean };
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
      const response = await fetch(`${apiBase}/api/supporter/status`, { headers: authRef.current });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setStatus(await response.json() as SupporterStatus);
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

  const write = async (url: string, init: RequestInit, verb: string, secret?: string) => {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(url, { ...init, headers: { ...authRef.current, ...init.headers } });
      if (!response.ok) {
        const body = await response.json().catch(() => null) as { error?: { message?: string } } | null;
        const message = body?.error?.message?.replace(secret ?? "\u0000", "[redacted]");
        setError(message || `Could not ${verb} (HTTP ${response.status}).`);
      }
    } catch {
      setError(`Could not reach the daemon to ${verb}.`);
    } finally {
      clearInput();
    }
    await refreshStatus();
    setBusy(false);
  };

  const saveToken = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const token = (tokenRef.current?.value ?? "").trim();
    if (!token || busy) return;
    await write(
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

  const setFeature = async (feature: "memory_rerank" | "tab_state", enabled: boolean) => {
    if (busy) return;
    await write(
      `${apiBase}/api/supporter/features`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ [feature]: enabled }),
      },
      `update ${feature === "memory_rerank" ? "memory re-rank" : "tab-state"}`,
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
    setFeature,
    select,
  };
}

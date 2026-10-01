import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";

interface AdapterTokenStatus {
  present: boolean;
  mode_ok: boolean | null;
}

interface DecisionStatus {
  adapter_registered: boolean;
  adapters: { id: string; token: AdapterTokenStatus }[];
}

export type TokenState = "set" | "not set";

// The token value lives only in the uncontrolled input until submit; it is never stored in state.
export function useDecisionToken(adapter: string, apiBase: string, apiHeaders: HeadersInit) {
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

  const write = async (init: RequestInit, url: string, verb: string) => {
    setBusy(true);
    setError(null);
    try {
      const r = await fetch(url, init);
      if (!r.ok) setError(`Could not ${verb} the token (HTTP ${r.status}).`);
    } catch {
      setError(`Could not reach the daemon to ${verb} the token.`);
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
      {
        method: "POST",
        headers: { "Content-Type": "application/json", ...authRef.current },
        body: JSON.stringify({ adapter, token }),
      },
      `${apiBase}/api/mem/decision/token`,
      "save",
    );
  };

  const clearToken = async () => {
    if (busy) return;
    await write(
      { method: "DELETE", headers: authRef.current },
      `${apiBase}/api/mem/decision/token?adapter=${encodeURIComponent(adapter)}`,
      "clear",
    );
  };

  const token = status?.adapters?.find((a) => a.id === adapter)?.token;
  return {
    tokenRef,
    busy,
    error,
    statusFailed,
    tokenState: (token ? (token.present ? "set" : "not set") : null) as TokenState | null,
    modeWarning: token?.present === true && token.mode_ok === false,
    adapterMissing: status !== null && !status.adapter_registered,
    saveToken,
    clearToken,
  };
}

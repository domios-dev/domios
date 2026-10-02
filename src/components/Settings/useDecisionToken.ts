import { useCallback, useEffect, useRef, useState } from "react";
import type { SupporterAdapter, SupporterFailure, SupporterMutation, SupporterStatus } from "./supporterTypes";
import { isSupporterCatalog, isSupporterStatus, redactSupporterError } from "./supporterResponse";
export type * from "./supporterTypes";

export function useDecisionToken(apiBase: string, apiHeaders: HeadersInit) {
  const [status, setStatus] = useState<SupporterStatus | null>(null);
  const [catalog, setCatalog] = useState<SupporterAdapter[]>([]);
  const [statusFailed, setStatusFailed] = useState(false);
  const [catalogFailed, setCatalogFailed] = useState(false);
  const [errors, setErrors] = useState<Record<string, SupporterFailure>>({});
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<Record<string, boolean>>({});
  const authRef = useRef(apiHeaders);
  const pending = useRef(false);
  const generation = useRef(0);
  authRef.current = apiHeaders;

  const refreshStatus = useCallback(async (): Promise<boolean> => {
    const current = ++generation.current;
    const results = await Promise.allSettled([
      fetch(`${apiBase}/api/supporter/status`, {
        headers: authRef.current,
      }).then(async (response) => {
        if (!response.ok) throw new Error("Status unavailable");
        const value: unknown = await response.json();
        if (!isSupporterStatus(value)) throw new Error("Status unavailable");
        return value;
      }),
      fetch(`${apiBase}/api/supporter/models`, {
        headers: authRef.current,
      }).then(async (response) => {
        if (!response.ok) throw new Error("Catalogue unavailable");
        const value: unknown = await response.json();
        if (!isSupporterCatalog(value)) throw new Error("Catalogue unavailable");
        return value;
      }),
    ]);
    if (generation.current !== current) return false;
    const [nextStatus, nextCatalog] = results;
    setStatusFailed(nextStatus.status === "rejected");
    setCatalogFailed(nextCatalog.status === "rejected");
    if (nextStatus.status === "fulfilled") setStatus(nextStatus.value);
    if (nextCatalog.status === "fulfilled") setCatalog(nextCatalog.value.adapters);
    return nextStatus.status === "fulfilled";
  }, [apiBase]);

  useEffect(() => {
    void refreshStatus();
    return () => {
      generation.current++;
    };
  }, [refreshStatus]);

  const write = async (
    adapter: string,
    path: string,
    method: string,
    body?: unknown,
    secret?: string,
  ): Promise<SupporterMutation | null> => {
    if (pending.current || statusFailed || !status) return null;
    pending.current = true;
    setBusy(true);
    setErrors((old) => {
      const next = { ...old };
      delete next[adapter];
      return next;
    });
    let result: SupporterMutation | null = null;
    try {
      const headers = new Headers(authRef.current);
      if (body !== undefined) headers.set("Content-Type", "application/json");
      const response = await fetch(`${apiBase}/api/supporter/${path}`, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await response.text();
      let parsed: SupporterMutation | null = null;
      try {
        const value: unknown = JSON.parse(text);
        if (typeof value === "object" && value !== null) parsed = value as SupporterMutation;
      } catch {
        /* Plain HTTP bodies are also actionable. */
      }
      if (!response.ok) {
        const message = parsed?.error?.message || text || `HTTP ${response.status}`;
        setErrors((old) => ({
          ...old,
          [adapter]: {
            code: parsed?.error?.code ?? "unavailable",
            message: redactSupporterError(message, secret),
          },
        }));
      } else if (
        parsed &&
        (path !== "connections" || parsed.state === "active" || parsed.state === "saved_not_active")
      ) {
        result = parsed;
        if (result.error)
          result.error = {
            ...result.error,
            message: redactSupporterError(result.error.message, secret),
          };
        setSaved((old) => ({
          ...old,
          [adapter]: result?.state === "saved_not_active",
        }));
        if (result.error)
          setErrors((old) => ({
            ...old,
            [adapter]: result?.error ?? {
              code: "unavailable",
              message: "Retry activation.",
            },
          }));
      } else {
        setErrors((old) => ({
          ...old,
          [adapter]: {
            code: "unavailable",
            message: "The daemon returned an unknown outcome. Refreshing status before retry.",
          },
        }));
      }
    } catch {
      setErrors((old) => ({
        ...old,
        [adapter]: {
          code: "unavailable",
          message: "The outcome is unknown. Refreshing authoritative status before retry.",
        },
      }));
    }
    await refreshStatus();
    pending.current = false;
    setBusy(false);
    return result;
  };
  const path = (adapter: string) => `connections/${encodeURIComponent(adapter)}`;
  const activate = (adapter: string) => write(adapter, `${path(adapter)}/activate`, "POST");
  const connect = (adapter: string, model: string, token?: string) =>
    write(adapter, "connections", "POST", { adapter, model, token, activate: true }, token);
  const selectModel = (adapter: string, model: string) => write(adapter, `${path(adapter)}/model`, "PUT", { model });
  const saveToken = async (adapter: string, token: string) =>
    (await write(adapter, `${path(adapter)}/token`, "POST", { token }, token)) !== null;
  const clearToken = async (adapter: string): Promise<boolean> =>
    (await write(adapter, `token?adapter=${encodeURIComponent(adapter)}`, "DELETE")) !== null;
  const disconnect = async (adapter: string): Promise<boolean> =>
    (await write(adapter, path(adapter), "DELETE")) !== null;
  const setBalance = async (adapter: string, balance: number | null) => {
    await write(adapter, "balance", "POST", { adapter, balance_usd: balance });
  };

  return {
    status,
    catalog,
    busy,
    errors,
    saved,
    statusFailed,
    catalogFailed,
    refreshStatus,
    activate,
    connect,
    selectModel,
    saveToken,
    clearToken,
    disconnect,
    setBalance,
  };
}

export type SupporterControls = ReturnType<typeof useDecisionToken>;

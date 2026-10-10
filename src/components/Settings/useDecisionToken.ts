import { useCallback, useEffect, useRef, useState } from "react";
import type { SupporterAdapter, SupporterFailure, SupporterMutation, SupporterStatus } from "./supporterTypes";
import { isSupporterCatalog, isSupporterStatus, redactSupporterError } from "./supporterResponse";
export type * from "./supporterTypes";

interface MutationOutcome {
  mutation: SupporterMutation | null;
  accepted: boolean;
  unknownResponse: boolean;
  refreshed: boolean;
  status: SupporterStatus | null;
}

interface DisconnectOutcome {
  confirmed: boolean;
  needsReconcile: boolean;
  selection: { adapter: string | null; model: string | null } | null;
}

function hasCoherentSelection(value: SupporterStatus) {
  const active = value.connections.filter((connection) => connection.active);
  if (!value.adapter) return active.length === 0 && !value.connected && value.state !== "active";
  if (active.length !== 1) return false;
  const selected = active[0];
  return !!selected && selected.adapter === value.adapter && selected.model === value.model;
}

export function useDecisionToken(apiBase: string, apiHeaders: HeadersInit) {
  const [status, setStatus] = useState<SupporterStatus | null>(null);
  const [catalog, setCatalog] = useState<SupporterAdapter[]>([]);
  const [statusFailed, setStatusFailed] = useState(false);
  const [catalogFailed, setCatalogFailed] = useState(false);
  const [errors, setErrors] = useState<Record<string, SupporterFailure>>({});
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [saved, setSaved] = useState<Record<string, boolean>>({});
  const authRef = useRef(apiHeaders);
  const pending = useRef(false);
  const refreshingCount = useRef(0);
  const statusRef = useRef<SupporterStatus | null>(null);
  const statusFresh = useRef(false);
  const generation = useRef(0);
  authRef.current = apiHeaders;

  const refreshStatus = useCallback(async (): Promise<boolean> => {
    const current = ++generation.current;
    refreshingCount.current++;
    setRefreshing(true);
    try {
      const results = await Promise.allSettled([
        fetch(`${apiBase}/api/supporter/status`, {
          headers: authRef.current,
        }).then(async (response) => {
          if (!response.ok) throw new Error("Status unavailable");
          const value: unknown = await response.json();
          if (!isSupporterStatus(value) || !hasCoherentSelection(value)) throw new Error("Status unavailable");
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
      const statusOk = nextStatus.status === "fulfilled";
      statusFresh.current = statusOk;
      setStatusFailed(!statusOk);
      setCatalogFailed(nextCatalog.status === "rejected");
      if (statusOk) {
        statusRef.current = nextStatus.value;
        setStatus(nextStatus.value);
        setErrors((old) =>
          Object.fromEntries(Object.entries(old).filter(([, error]) => error.code !== "outcome_unconfirmed")),
        );
      }
      if (nextCatalog.status === "fulfilled") setCatalog(nextCatalog.value.adapters);
      return statusOk;
    } finally {
      refreshingCount.current--;
      setRefreshing(refreshingCount.current > 0);
    }
  }, [apiBase]);

  useEffect(() => {
    void refreshStatus();
    return () => {
      generation.current++;
    };
  }, [refreshStatus]);

  const setOutcomeError = (adapter: string, message: string) =>
    setErrors((old) => ({ ...old, [adapter]: { code: "outcome_unconfirmed", message } }));

  const write = async (
    adapter: string,
    path: string,
    method: string,
    body?: unknown,
    secret?: string,
  ): Promise<MutationOutcome | null> => {
    if (pending.current || refreshingCount.current > 0 || !statusFresh.current || !statusRef.current) return null;
    pending.current = true;
    setBusy(true);
    setErrors((old) => {
      const next = { ...old };
      delete next[adapter];
      return next;
    });
    let mutation: SupporterMutation | null = null;
    let accepted = false;
    let unknownResponse = false;
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
        accepted = true;
        const acceptedMutation = parsed.error
          ? { ...parsed, error: { ...parsed.error, message: redactSupporterError(parsed.error.message, secret) } }
          : parsed;
        mutation = acceptedMutation;
        const acceptedError = acceptedMutation.error;
        if (acceptedError)
          setErrors((old) => ({
            ...old,
            [adapter]: acceptedError,
          }));
      } else {
        unknownResponse = true;
        setErrors((old) => ({
          ...old,
          [adapter]: {
            code: "outcome_unconfirmed",
            message: "The daemon returned an unknown outcome. Refreshing status before retry.",
          },
        }));
      }
    } catch {
      unknownResponse = true;
      setErrors((old) => ({
        ...old,
        [adapter]: {
          code: "outcome_unconfirmed",
          message: "The outcome is unknown. Refreshing authoritative status before retry.",
        },
      }));
    }
    let refreshed = false;
    try {
      refreshed = await refreshStatus();
    } finally {
      pending.current = false;
      setBusy(false);
    }
    const currentStatus = refreshed ? statusRef.current : null;
    if (!refreshed && accepted) {
      setOutcomeError(adapter, "The server accepted the change, but its current state could not be confirmed. Refresh before retrying.");
    } else if (!refreshed && unknownResponse) {
      setOutcomeError(adapter, "The outcome is unknown. Refresh authoritative status before retrying.");
    }
    if (accepted && currentStatus && mutation?.state) {
      const connection = currentStatus.connections.find((entry) => entry.adapter === adapter);
      if (mutation.state === "saved_not_active" && connection && !connection.active) {
        setSaved((old) => ({ ...old, [adapter]: true }));
      } else if (mutation.state === "active" && connection?.active && currentStatus.adapter === adapter) {
        setSaved((old) => ({ ...old, [adapter]: false }));
      }
    }
    return {
      mutation: accepted && refreshed ? mutation : null,
      accepted,
      unknownResponse,
      refreshed,
      status: currentStatus,
    };
  };
  const path = (adapter: string) => `connections/${encodeURIComponent(adapter)}`;
  const activate = async (adapter: string): Promise<boolean> => {
    const outcome = await write(adapter, `${path(adapter)}/activate`, "POST");
    if (!outcome?.refreshed || !outcome.status) return false;
    if (outcome.mutation?.state === "saved_not_active") return false;
    const connection = outcome.status.connections.find((entry) => entry.adapter === adapter);
    if (outcome.status.adapter === adapter && connection?.active) return true;
    if (outcome.accepted || outcome.unknownResponse) {
      setOutcomeError(adapter, "The activation outcome was not confirmed by refreshed status. Review status before retrying.");
    }
    return false;
  };
  const connect = async (adapter: string, model: string, token?: string): Promise<SupporterMutation | null> => {
    const activeBefore = statusRef.current?.adapter ?? null;
    const modelBefore = statusRef.current?.model ?? null;
    const outcome = await write(adapter, "connections", "POST", { adapter, model, token, activate: true }, token);
    if (!outcome?.refreshed || !outcome.status) return null;
    const connection = outcome.status.connections.find((entry) => entry.adapter === adapter);
    const confirmedActive = outcome.status.adapter === adapter && outcome.status.model === model && connection?.active;
    const confirmedSaved =
      !!connection &&
      !connection.active &&
      outcome.status.adapter === activeBefore &&
      outcome.status.model === modelBefore;
    if (outcome.accepted) {
      if (outcome.mutation?.state === "active" && confirmedActive) return outcome.mutation;
      if (outcome.mutation?.state === "saved_not_active" && confirmedSaved) return outcome.mutation;
      setOutcomeError(adapter, "The connection outcome was not confirmed by refreshed status. Review status before retrying.");
      return null;
    }
    if (outcome.unknownResponse) {
      if (confirmedActive) return { adapter, state: "active" };
      if (confirmedSaved) return { adapter, state: "saved_not_active" };
      setOutcomeError(adapter, "The connection outcome was not confirmed by refreshed status. Review status before retrying.");
      return null;
    }
    return null;
  };
  const selectModel = async (adapter: string, model: string): Promise<boolean> => {
    const outcome = await write(adapter, `${path(adapter)}/model`, "PUT", { model });
    if (!outcome?.refreshed || !outcome.status) return false;
    const connection = outcome.status.connections.find((entry) => entry.adapter === adapter);
    if (outcome.status.adapter === adapter && connection?.active && connection.model === model) return true;
    if (outcome.accepted || outcome.unknownResponse) {
      setOutcomeError(adapter, "The model change outcome was not confirmed by refreshed status. Review status before retrying.");
    }
    return false;
  };
  const saveToken = async (adapter: string, token: string): Promise<boolean> => {
    const activeBefore = statusRef.current?.adapter ?? null;
    const modelBefore = statusRef.current?.model ?? null;
    const outcome = await write(adapter, `${path(adapter)}/token`, "POST", { token }, token);
    if (!outcome?.refreshed || !outcome.status) return false;
    if (!outcome.accepted) {
      if (outcome.unknownResponse) {
        setOutcomeError(adapter, "The credential repair outcome is unknown. Refreshed status cannot confirm which token was saved.");
      }
      return false;
    }
    const connection = outcome.status.connections.find((entry) => entry.adapter === adapter);
    if (
      outcome.status.adapter === activeBefore &&
      outcome.status.model === modelBefore &&
      connection?.token.present &&
      connection.token.mode_ok !== false
    ) return true;
    setOutcomeError(adapter, "Credential repair was accepted, but refreshed status does not confirm it. Review status before retrying.");
    return false;
  };
  const clearToken = async (adapter: string): Promise<boolean> => {
    const activeBefore = statusRef.current?.adapter ?? null;
    const modelBefore = statusRef.current?.model ?? null;
    const outcome = await write(adapter, `token?adapter=${encodeURIComponent(adapter)}`, "DELETE");
    if (!outcome?.refreshed || !outcome.status) return false;
    const connection = outcome.status.connections.find((entry) => entry.adapter === adapter);
    if (
      (outcome.accepted || outcome.unknownResponse) &&
      outcome.status.adapter === activeBefore &&
      outcome.status.model === modelBefore &&
      connection &&
      !connection.token.present
    ) return true;
    if (outcome.accepted || outcome.unknownResponse) {
      setOutcomeError(adapter, "The token change outcome was not confirmed by refreshed status. Review status before retrying.");
    }
    return false;
  };
  const disconnect = async (adapter: string): Promise<DisconnectOutcome> => {
    const activeBefore = statusRef.current?.adapter ?? null;
    const modelBefore = statusRef.current?.model ?? null;
    const outcome = await write(adapter, path(adapter), "DELETE");
    if (!outcome) return { confirmed: false, needsReconcile: false, selection: null };
    if (!outcome.refreshed || !outcome.status) {
      return { confirmed: false, needsReconcile: true, selection: { adapter: activeBefore, model: modelBefore } };
    }
    const remaining = outcome.status.connections.some((entry) => entry.adapter === adapter);
    if (!remaining) {
      setErrors((old) => {
        const next = { ...old };
        delete next[adapter];
        return next;
      });
      return {
        confirmed: true,
        needsReconcile: false,
        selection: { adapter: outcome.status.adapter, model: outcome.status.model },
      };
    }
    if (outcome.accepted || outcome.unknownResponse) {
      setOutcomeError(adapter, "The removal outcome was not confirmed by refreshed status. Review current state before retrying.");
    }
    return {
      confirmed: false,
      needsReconcile: false,
      selection: { adapter: outcome.status.adapter, model: outcome.status.model },
    };
  };
  const isBusy = busy || refreshing;

  return {
    status,
    catalog,
    busy: isBusy,
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
  };
}

export type SupporterControls = ReturnType<typeof useDecisionToken>;

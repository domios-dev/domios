import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ApiConfig } from "../lib/types";
import { useConfig } from "./useConfig";

export type UpdateStatus =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "applying"
  | "error";

export type UpdateChannel = "stable" | "rc" | "nightly";

export interface UpdateInfo {
  current_version: string;
  latest_version: string;
  update_available: boolean;
  release_url?: string;
  release_notes?: string | null;
  download_url?: string | null;
  published_at?: string | null;
}

interface InstallResponse {
  status: "updating" | "up_to_date" | "command" | "browser_opened";
  message?: string;
  version?: string;
}

function baseUrl(api: ApiConfig): string {
  const host = api.host === "0.0.0.0" ? "127.0.0.1" : api.host;
  return `http://${host}:${api.port}`;
}

function authHeaders(api: ApiConfig): HeadersInit {
  return api.token ? { Authorization: `Bearer ${api.token}` } : {};
}

function isUpdateInfo(v: unknown): v is UpdateInfo {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.current_version === "string" &&
    typeof o.latest_version === "string" &&
    typeof o.update_available === "boolean"
  );
}

function isInstallResponse(v: unknown): v is InstallResponse {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return typeof o.status === "string";
}

function isChannelResponse(v: unknown): v is { channel: UpdateChannel } {
  if (typeof v !== "object" || v === null) return false;
  const c = (v as { channel?: unknown }).channel;
  return c === "stable" || c === "rc" || c === "nightly";
}

export function useUpdate() {
  const { config } = useConfig();
  const api = config.api;
  const apiRef = useRef(api);
  apiRef.current = api;

  const [status, setStatus] = useState<UpdateStatus>("idle");
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [channel, setChannelState] = useState<UpdateChannel | null>(null);
  const [channelSupported, setChannelSupported] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);

  const handleInfo = useCallback((next: UpdateInfo) => {
    setInfo(next);
    setStatus(next.update_available ? "available" : "up-to-date");
  }, []);

  const refresh = useCallback(
    async (force = false): Promise<void> => {
      const a = apiRef.current;
      setStatus("checking");
      setError(null);
      try {
        const url = `${baseUrl(a)}/api/update/check${force ? "?force=true" : ""}`;
        const res = await fetch(url, { headers: authHeaders(a) });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data: unknown = await res.json();
        if (!isUpdateInfo(data)) throw new Error("Malformed update info");
        handleInfo(data);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        setStatus("error");
      }
    },
    [handleInfo],
  );

  const fetchChannel = useCallback(async (): Promise<void> => {
    const a = apiRef.current;
    try {
      const res = await fetch(`${baseUrl(a)}/api/update/channel`, {
        headers: authHeaders(a),
      });
      if (res.status === 404) {
        setChannelSupported(false);
        return;
      }
      if (!res.ok) return;
      const data: unknown = await res.json();
      if (isChannelResponse(data)) setChannelState(data.channel);
    } catch {
      setChannelSupported(false);
    }
  }, []);

  const setChannel = useCallback(
    async (next: UpdateChannel): Promise<void> => {
      const a = apiRef.current;
      const prev = channel;
      setChannelState(next);
      try {
        const res = await fetch(`${baseUrl(a)}/api/update/channel`, {
          method: "POST",
          headers: { ...authHeaders(a), "Content-Type": "application/json" },
          body: JSON.stringify({ channel: next }),
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        await refresh(true);
      } catch (e) {
        setChannelState(prev);
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [channel, refresh],
  );

  const applyUpdate = useCallback(async (): Promise<InstallResponse | null> => {
    const a = apiRef.current;
    setStatus("applying");
    setError(null);
    try {
      const res = await fetch(
        `${baseUrl(a)}/api/update/install?method=auto`,
        { method: "POST", headers: authHeaders(a) },
      );
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data: unknown = await res.json();
      if (!isInstallResponse(data)) throw new Error("Malformed install response");
      return data;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setStatus("error");
      return null;
    }
  }, []);

  const mountedRef = useRef(false);
  useEffect(() => {
    if (mountedRef.current) return;
    mountedRef.current = true;
    void refresh(false);
    void fetchChannel();
  }, [refresh, fetchChannel]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    listen<unknown>("update-available", (event) => {
      if (cancelled) return;
      if (isUpdateInfo(event.payload)) handleInfo(event.payload);
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleInfo]);

  return {
    status,
    info,
    currentVersion: info?.current_version ?? null,
    latestVersion: info?.latest_version ?? null,
    releaseNotes: info?.release_notes ?? null,
    releaseUrl: info?.release_url ?? null,
    channel,
    channelSupported,
    setChannel,
    refresh,
    applyUpdate,
    applying: status === "applying",
    error,
  };
}

import {
  createContext,
  createElement,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { listen } from "../lib/bridge";
import type { ApiConfig } from "../lib/types";
import { useConfig } from "./useConfig";

export type UpdateStatus =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "applying"
  | "restarting"
  | "error";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "downloading"
  | "applying"
  | "success"
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

interface RuntimeStatus {
  phase: UpdatePhase;
  last_error: string | null;
  channel: UpdateChannel;
  install_method: string;
  current_version: string;
  latest_known: string | null;
  update_available: boolean;
  phase_age_secs: number | null;
  downloaded_bytes: number;
  download_total_bytes: number | null;
  download_progress: number | null;
}

interface UpdateContextValue {
  status: UpdateStatus;
  phase: UpdatePhase;
  info: UpdateInfo | null;
  currentVersion: string | null;
  latestVersion: string | null;
  releaseNotes: string | null;
  releaseUrl: string | null;
  channel: UpdateChannel | null;
  channelSupported: boolean;
  progress: number;
  downloadedBytes: number;
  downloadTotalBytes: number | null;
  toastOpen: boolean;
  blocking: boolean;
  installing: boolean;
  setChannel: (next: UpdateChannel) => Promise<void>;
  refresh: (force?: boolean, opts?: { silent?: boolean }) => Promise<void>;
  applyUpdate: () => Promise<InstallResponse | null>;
  dismissUpdateToast: () => void;
  error: string | null;
}

const UpdateContext = createContext<UpdateContextValue | null>(null);

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

function isUpdateChannel(v: unknown): v is UpdateChannel {
  return v === "stable" || v === "rc" || v === "nightly";
}

function isChannelResponse(v: unknown): v is { channel: UpdateChannel } {
  if (typeof v !== "object" || v === null) return false;
  return isUpdateChannel((v as { channel?: unknown }).channel);
}

function isUpdatePhase(v: unknown): v is UpdatePhase {
  return (
    v === "idle" ||
    v === "checking" ||
    v === "downloading" ||
    v === "applying" ||
    v === "success" ||
    v === "error"
  );
}

function nullableString(v: unknown): string | null {
  return typeof v === "string" ? v : null;
}

function nullableNumber(v: unknown): number | null {
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

function numberOrZero(v: unknown): number {
  return typeof v === "number" && Number.isFinite(v) ? v : 0;
}

function isRuntimeStatus(v: unknown): v is RuntimeStatus {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return isUpdatePhase(o.phase) && typeof o.current_version === "string";
}

function normalizeRuntimeStatus(v: RuntimeStatus): RuntimeStatus {
  return {
    phase: v.phase,
    last_error: nullableString(v.last_error),
    channel: isUpdateChannel(v.channel) ? v.channel : "stable",
    install_method: typeof v.install_method === "string" ? v.install_method : "unknown",
    current_version: v.current_version,
    latest_known: nullableString(v.latest_known),
    update_available: v.update_available === true,
    phase_age_secs: nullableNumber(v.phase_age_secs),
    downloaded_bytes: numberOrZero(v.downloaded_bytes),
    download_total_bytes: nullableNumber(v.download_total_bytes),
    download_progress: nullableNumber(v.download_progress),
  };
}

function progressForRuntime(next: RuntimeStatus, previous: number): number {
  const age = next.phase_age_secs ?? 0;
  switch (next.phase) {
    case "checking":
      return Math.max(previous, Math.min(18, 8 + age * 2.5));
    case "downloading": {
      if (next.download_total_bytes && next.download_total_bytes > 0) {
        const ratio = Math.min(1, next.downloaded_bytes / next.download_total_bytes);
        return Math.max(previous, Math.round(18 + ratio * 62));
      }
      return Math.max(previous, Math.min(78, previous + 3));
    }
    case "applying":
      return Math.max(previous, Math.min(94, 82 + age * 2));
    case "success":
      return 100;
    case "idle":
      return 0;
    case "error":
      return previous;
  }
}

function phaseToStatus(phase: UpdatePhase, installing: boolean): UpdateStatus | null {
  switch (phase) {
    case "checking":
      return installing ? "checking" : null;
    case "downloading":
      return "downloading";
    case "applying":
      return "applying";
    case "success":
      return "restarting";
    case "error":
      return "error";
    case "idle":
      return null;
  }
}

export function UpdateProvider({ children }: { children: ReactNode }) {
  const { config } = useConfig();
  const api = config.api;
  const apiRef = useRef(api);
  apiRef.current = api;

  const [status, setStatus] = useState<UpdateStatus>("idle");
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [channel, setChannelState] = useState<UpdateChannel | null>(null);
  const [channelSupported, setChannelSupported] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [toastOpen, setToastOpen] = useState(false);
  const [progress, setProgress] = useState(0);
  const [downloadedBytes, setDownloadedBytes] = useState(0);
  const [downloadTotalBytes, setDownloadTotalBytes] = useState<number | null>(null);
  const installingRef = useRef(false);
  const toastOpenRef = useRef(false);

  useEffect(() => {
    installingRef.current = installing;
  }, [installing]);

  useEffect(() => {
    toastOpenRef.current = toastOpen;
  }, [toastOpen]);

  const handleInfo = useCallback((next: UpdateInfo) => {
    setInfo(next);
    if (installingRef.current) return;
    setStatus(next.update_available ? "available" : "up-to-date");
  }, []);

  const refresh = useCallback(
    async (force = false, opts?: { silent?: boolean }): Promise<void> => {
      if (installingRef.current) return;
      const a = apiRef.current;
      const silent = opts?.silent ?? false;
      if (!silent) setStatus("checking");
      setError(null);
      try {
        const url = `${baseUrl(a)}/api/update/check${force ? "?force=true" : ""}`;
        const res = await fetch(url, { headers: authHeaders(a) });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data: unknown = await res.json();
        if (!isUpdateInfo(data)) throw new Error("Malformed update info");
        handleInfo(data);
      } catch (e) {
        if (silent) return;
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

  const applyRuntimeStatus = useCallback(
    (raw: RuntimeStatus, opts?: { adopt?: boolean }) => {
      const next = normalizeRuntimeStatus(raw);
      const activePhase =
        next.phase === "checking" ||
        next.phase === "downloading" ||
        next.phase === "applying" ||
        next.phase === "success";
      const shouldAdopt =
        opts?.adopt === true &&
        (next.phase === "downloading" || next.phase === "applying" || next.phase === "success");
      const nextInstalling = installingRef.current || shouldAdopt || (activePhase && toastOpenRef.current);

      setPhase(next.phase);
      setChannelState(next.channel);
      setDownloadedBytes(next.downloaded_bytes);
      setDownloadTotalBytes(next.download_total_bytes);

      if (next.latest_known) {
        setInfo((prev) => ({
          current_version: prev?.current_version ?? next.current_version,
          latest_version: next.latest_known ?? prev?.latest_version ?? next.current_version,
          update_available: next.update_available,
          release_url: prev?.release_url,
          release_notes: prev?.release_notes,
          download_url: prev?.download_url,
          published_at: prev?.published_at,
        }));
      }

      if (shouldAdopt) {
        setInstalling(true);
        setToastOpen(true);
      }

      if (next.phase === "error") {
        const message = next.last_error || "Update failed";
        setError(message);
        setStatus("error");
        setInstalling(false);
        setToastOpen(true);
        return;
      }

      setError(null);
      setProgress((prev) => progressForRuntime(next, prev));

      const runtimeStatus = phaseToStatus(next.phase, nextInstalling);
      if (runtimeStatus) {
        setStatus(runtimeStatus);
        return;
      }

      if (next.phase === "idle" && installingRef.current) {
        setInstalling(false);
        setToastOpen(false);
        setProgress(0);
        setStatus(next.update_available ? "available" : "up-to-date");
      }
    },
    [],
  );

  const fetchRuntimeStatus = useCallback(
    async (opts?: { adopt?: boolean }): Promise<RuntimeStatus | null> => {
      const a = apiRef.current;
      const res = await fetch(`${baseUrl(a)}/api/update/status`, {
        headers: authHeaders(a),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data: unknown = await res.json();
      if (!isRuntimeStatus(data)) throw new Error("Malformed update status");
      applyRuntimeStatus(data, opts);
      return normalizeRuntimeStatus(data);
    },
    [applyRuntimeStatus],
  );

  const applyUpdate = useCallback(async (): Promise<InstallResponse | null> => {
    const a = apiRef.current;
    setInstalling(true);
    setToastOpen(true);
    setStatus("checking");
    setPhase("checking");
    setProgress((prev) => Math.max(prev, 8));
    setError(null);
    try {
      const res = await fetch(`${baseUrl(a)}/api/update/install?method=auto`, {
        method: "POST",
        headers: authHeaders(a),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data: unknown = await res.json();
      if (!isInstallResponse(data)) throw new Error("Malformed install response");
      if (data.status === "up_to_date") {
        setInstalling(false);
        setToastOpen(false);
        setProgress(0);
        await refresh(true, { silent: true });
      }
      return data;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setStatus("error");
      setPhase("error");
      setInstalling(false);
      setToastOpen(true);
      return null;
    }
  }, [refresh]);

  const dismissUpdateToast = useCallback(() => {
    if (status === "applying" || status === "restarting") return;
    setToastOpen(false);
  }, [status]);

  useEffect(() => {
    void refresh(false, { silent: true });
    void fetchChannel();
    void fetchRuntimeStatus({ adopt: true }).catch(() => {});

    const interval = setInterval(() => void refresh(false, { silent: true }), 30 * 60 * 1000);
    return () => clearInterval(interval);
  }, [fetchChannel, fetchRuntimeStatus, refresh]);

  useEffect(() => {
    if (!installing) return;
    let cancelled = false;
    const poll = async () => {
      try {
        const next = await fetchRuntimeStatus();
        if (!cancelled && next?.phase === "idle") {
          await refresh(true, { silent: true });
        }
      } catch (e) {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
        setStatus("error");
        setPhase("error");
        setInstalling(false);
        setToastOpen(true);
      }
    };

    void poll();
    const interval = setInterval(() => void poll(), 750);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [fetchRuntimeStatus, installing, refresh]);

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

  const blocking = status === "applying" || status === "restarting";

  const value = useMemo<UpdateContextValue>(
    () => ({
      status,
      phase,
      info,
      currentVersion: info?.current_version ?? null,
      latestVersion: info?.latest_version ?? null,
      releaseNotes: info?.release_notes ?? null,
      releaseUrl: info?.release_url ?? null,
      channel,
      channelSupported,
      progress,
      downloadedBytes,
      downloadTotalBytes,
      toastOpen,
      blocking,
      installing,
      setChannel,
      refresh,
      applyUpdate,
      dismissUpdateToast,
      error,
    }),
    [
      applyUpdate,
      blocking,
      channel,
      channelSupported,
      dismissUpdateToast,
      downloadedBytes,
      downloadTotalBytes,
      error,
      info,
      installing,
      phase,
      progress,
      refresh,
      setChannel,
      status,
      toastOpen,
    ],
  );

  return createElement(UpdateContext.Provider, { value }, children);
}

export function useUpdate() {
  const ctx = useContext(UpdateContext);
  if (!ctx) throw new Error("useUpdate must be used within UpdateProvider");
  return ctx;
}

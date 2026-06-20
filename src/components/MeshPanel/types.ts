export type ServiceKind = "cron" | "daemon";
export type HealthMode = "http" | "tcp" | "process" | "script";
export type HealthStatus = "healthy" | "degraded" | "down" | "unknown";
export type ActivitySource = "push" | "health" | "peer" | "cron";
export type ActivityLevel = "info" | "warn" | "error";

export interface HealthSnapshot {
  status: HealthStatus;
  probed_at?: number;
  latency_ms?: number;
  version?: string;
  uptime_secs?: number;
  extra?: Record<string, unknown>;
}

export interface Service {
  id: string;
  name: string;
  kind: ServiceKind;
  description?: string;
  host?: string;
  url?: string;
  port?: number;
  process_label?: string;
  script_path?: string;
  health_mode: HealthMode;
  health_path?: string;
  health_interval: number;
  health: HealthSnapshot;
  push_enabled: boolean;
  schedule?: string;
  next_run?: number;
  last_run_status?: string;
  registered_at: number;
  updated_at: number;
  enabled: boolean;
  // Populated client-side when merging from remote peers
  source_peer_id?: string;
  source_peer?: string;
  /** Last-known data served from the daemon cache (peer unreachable). */
  stale?: boolean;
  /** Online state of the owning peer at merge time. */
  peer_online?: boolean;
}

export interface ActivityEvent {
  id?: number;
  occurred_at: number;
  source: ActivitySource;
  source_id?: string;
  target?: string;
  source_peer?: string;
  level: ActivityLevel;
  message: string;
  meta: Record<string, unknown>;
}

export type MeshTab = "map" | "activity" | "network";

export const CRON_ID_PREFIX = "cron_";

export function serviceIdToCronId(serviceId: string): string {
  return serviceId.startsWith(CRON_ID_PREFIX)
    ? serviceId.slice(CRON_ID_PREFIX.length)
    : serviceId;
}

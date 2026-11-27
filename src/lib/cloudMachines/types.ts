import type { MeshPeersResponse } from "../../../gen/MeshPeersResponse";
import type { PairedPeer } from "../../../gen/PairedPeer";
import type {
  CloudStorageTier,
  CloudStorageUsageSummaryWire,
} from "../../components/Usage/cloudStorageModel";

export type CloudLifecycleState =
  | "creating"
  | "enrolling"
  | "awake"
  | "draining"
  | "asleep"
  | "waking"
  | "out_of_credits"
  | "failed"
  | "deleted";

export type CloudSleepMode = "pause" | "stop" | "stop_billed" | "delete_recreate";

export interface CloudRate {
  amount: number;
  currency: string;
  credits_per_hour?: number | null;
}

export type CloudEndpoint =
  | { kind: "dedicated_ipv4"; ipv4: string; port: number }
  | { kind: "provider_proxy"; host: string; port: number; scheme: string }
  | string;

export interface CloudCostSummary {
  kind: "cloud";
  machine_id: string;
  name: string;
  provider: string;
  size: string;
  rate_per_hour?: CloudRate | null;
  state: CloudLifecycleState;
  sleep_mode: CloudSleepMode;
  uptime_secs: number;
  billing_notice: string;
}

export interface CloudReceipt {
  kind: "cloud";
  machine_id: string;
  name: string;
  provider: string;
  size: string;
  rate_per_hour?: CloudRate | null;
  state: CloudLifecycleState;
  sleep_mode: CloudSleepMode;
  uptime_secs: number;
  installation_id?: string | null;
  peer_instance_id?: string | null;
  endpoint?: CloudEndpoint | null;
  lease_expires_at_ms?: number | null;
  storage_tier?: CloudStorageTier | null;
  created_at_ms?: number | null;
  last_seen_at_ms?: number | null;
  billing_notice: string;
  warning?: string | null;
}

export interface CloudSizeCatalogEntry {
  id: string;
  cpu?: number | null;
  ram_gb?: number | null;
  hourly_rate_cents: number;
  credits_per_hour: number;
}

export interface CloudCreditPackOption {
  pack_id: string;
  price_cents: number;
  hours: number;
  recommended?: boolean;
}

export interface CloudUsagePayload {
  warnings?: {
    epoch: number;
    pct80_at?: number;
    pct95_at?: number;
    exhausted_at?: number;
  } | null;
  used: number;
  allowance: number;
  purchased: number;
  remaining: number;
  cap: number;
  burn_per_size: Record<string, number>;
  currency?: string | null;
  period_end_at?: number | null;
  custom_min_cents?: number | null;
  custom_max_cents?: number | null;
  plan?: string | null;
  sizes?: CloudSizeCatalogEntry[] | null;
  packs?: CloudCreditPackOption[] | null;
  purchased_expiry_days?: number | null;
  allowance_hours?: Partial<Record<string, number>> | null;
  storage?: CloudStorageUsageSummaryWire | null;
}

export interface CloudPromoRedemption {
  schema_version: number;
  status: string;
  already_redeemed: boolean;
  hours: number;
  expires_at: number | null;
  remaining: number;
  purchased: number;
}

export interface CloudPackCheckoutReply {
  url: string;
  session_id: string;
  pack_id: string;
  credits: number;
  amount_cents: number;
  currency: string;
}

export type EnrichedPairedPeer = PairedPeer & {
  kind?: string;
  cloud?: CloudCostSummary | null;
};

export type EnrichedMeshPeersResponse = Omit<MeshPeersResponse, "paired"> & {
  paired: EnrichedPairedPeer[];
  cloud_machines?: CloudReceipt[];
};

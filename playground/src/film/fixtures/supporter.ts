import type { SupporterStatus } from "../../../../src/components/Settings/supporterTypes";
import { ago } from "./clock";
import { filmRoute } from "./daemon";

const SPEND_ZERO = { input_tokens: 0, output_tokens: 0, estimated_usd: 0 };

/** Spinal Cord connected through OpenRouter on Haiku 5, mid-afternoon reflex traffic. */
export const SUPPORTER: SupporterStatus = {
  state: "active", token_mode: "required", adapter: "openrouter", model: "haiku-5", deadline_ms: 4_000, connected: true, last_activity: ago(1),
  calls_last_hour: 23, calls_last_day: 188, last_verdict: {},
  budget: { reached: false, calls_per_hour: 120, free_at: null, storage_ready: true },
  pricing: { display_name: "Haiku 5", input_usd_per_million: 0.8, output_usd_per_million: 4 },
  spend: { today: { input_tokens: 512_000, output_tokens: 11_400, estimated_usd: 0.4552 }, month: SPEND_ZERO, exhausted: false, next_probe_at: null, storage_ready: true },
  token: { adapter: "openrouter", token_mode: "required", present: true, mode_ok: true },
  adapter_registered: true, adapters: [],
  connections: [{ adapter: "openrouter", model: "haiku-5", active: true, token_mode: "required", token: { present: true, mode_ok: true } }],
};

filmRoute("GET", "/api/supporter/status", () => SUPPORTER);
filmRoute("GET", "/api/supporter/activity", () => ({ entries: [] }));

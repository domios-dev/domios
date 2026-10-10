import "../../../../src/components/Sidebar/sidebarPanel.css";
import type { SupporterStatus } from "../../../../src/components/Settings/supporterTypes";
import { SidebarDisclosure } from "../../../../src/components/Sidebar/SidebarDisclosure";
import { SpinalCordButton } from "../../../../src/components/Sidebar/SpinalCord";
import type { SupporterActivity } from "../../../../src/components/Supporter/activity";
import type { ApiConfig } from "../../../../src/lib/types";
import { httpApi, mockDaemon } from "../fixtures";
import { ForcePseudo, PinnedMenu } from "../stageTools";
import { defineSpecimen } from "../types";

const SPEND_ZERO = { input_tokens: 0, output_tokens: 0, estimated_usd: 0 };

function status(overrides: Partial<SupporterStatus>): SupporterStatus {
  return {
    state: "not_connected", token_mode: "required", adapter: null, model: null, deadline_ms: 4_000, connected: false, last_activity: null,
    calls_last_hour: 0, calls_last_day: 0, last_verdict: {},
    budget: { reached: false, calls_per_hour: 120, free_at: null, storage_ready: true },
    pricing: null,
    spend: { today: SPEND_ZERO, month: SPEND_ZERO, exhausted: false, next_probe_at: null, storage_ready: true },
    token: { adapter: "openrouter", token_mode: "required", present: false },
    adapter_registered: true, adapters: [], connections: [],
    ...overrides,
  };
}

const CONNECTED = status({
  state: "active", adapter: "openrouter", model: "haiku-5", connected: true, last_activity: Date.now() - 40_000,
  calls_last_hour: 18, calls_last_day: 146,
  pricing: { display_name: "Haiku 5", input_usd_per_million: 0.8, output_usd_per_million: 4 },
  spend: { today: { input_tokens: 412_000, output_tokens: 9_800, estimated_usd: 0.3688 }, month: { input_tokens: 8_900_000, output_tokens: 214_000, estimated_usd: 7.98 }, exhausted: false, next_probe_at: null, storage_ready: true },
  token: { adapter: "openrouter", token_mode: "required", present: true, mode_ok: true },
  connections: [{ adapter: "openrouter", model: "haiku-5", active: true, token_mode: "required", token: { present: true, mode_ok: true } }],
});

const now = Math.floor(Date.now() / 1000);

function reflex(minutesAgo: number, kind: string, label: string, action: string, reason: string, extra: Partial<SupporterActivity> = {}): SupporterActivity {
  return { time: now - minutesAgo * 60, team_id: null, kind, label, confidence_band: "strong", action, reason, budget_reached: false, fallback: false, ...extra };
}

const REFLEXES: SupporterActivity[] = [
  reflex(1, "rerank", "ranked", "rerank", "all_candidates_scored"),
  reflex(4, "tab_state", "working_normally", "suppress", "healthy_snapshot"),
  reflex(9, "heartbeat", "nothing_changed", "suppress", "same_snapshot"),
  reflex(16, "recovery", "crashed_to_shell", "recovery", "confirmed_provider_exit_to_live_shell", { confidence_band: "weak" }),
  reflex(31, "tab_state", "trust_or_permission_prompt", "notify_only", "model_deadline", { fallback: true }),
  reflex(52, "sentinel", "action_needed", "wake", "hard_wake", { asked: [{ name: "deploy", question: "Did the nightly release train finish?", answer: "No, step 3 failed" }] }),
];

const apis = { off: httpApi("spinal-off"), on: httpApi("spinal-on"), quiet: httpApi("spinal-quiet") };
mockDaemon(apis.off, "/api/supporter/status", () => status({}));
mockDaemon(apis.on, "/api/supporter/status", () => CONNECTED);
mockDaemon(apis.on, "/api/supporter/activity", () => ({ entries: REFLEXES }));
mockDaemon(apis.quiet, "/api/supporter/status", () => ({ ...CONNECTED, calls_last_hour: 0, calls_last_day: 0, pricing: null }));
mockDaemon(apis.quiet, "/api/supporter/activity", () => ({ entries: [] }));

const BUTTON = '[data-testid="spinal-cord-button"]';

function SpinalSpecimen({ api, open = false, hover = false }: { api: ApiConfig; open?: boolean; hover?: boolean }) {
  const header = <div className="gallery-sidebar">
    <section className="sidebar-general-section" aria-label="General workspace contents">
      <div className="sidebar-structural-header sidebar-general-header w-full">
        <SidebarDisclosure className="sidebar-structural-toggle min-w-0" expanded>
          <span className="sidebar-structural-label">GENERAL</span>
        </SidebarDisclosure>
        <SpinalCordButton api={api} />
      </div>
    </section>
  </div>;
  if (open) return <PinnedMenu trigger={BUTTON} via="click" menuSelector='[data-testid="spinal-cord-popover"]'>{header}</PinnedMenu>;
  if (hover) return <ForcePseudo selector={BUTTON} state="hover">{header}</ForcePseudo>;
  return <div className="gallery-fill">{header}</div>;
}

export default defineSpecimen({
  id: "spinal-cord",
  title: "Spinal Cord",
  category: "Sidebar",
  where: "src/components/Sidebar/SpinalCord.tsx · src/components/Usage/SupporterAllowanceRow.tsx · src/components/Supporter/ActivityFeed.tsx",
  stage: "wide",
  notes: "The Spinal Cord entry on the General line of the sidebar. Not connected it is a quiet vertebrae invite whose popover sells the reflexes with the animated graphic and an Ask the General button; connected it turns green and the popover shows spend tiles, the live reflex count and the recent reflexes feed. Status and activity come from mocked GET /api/supporter/status and /api/supporter/activity, fetched through useSupporterResource.",
  Component: SpinalSpecimen,
  states: [
    { id: "off", label: "Not connected · invite", props: { api: apis.off } },
    { id: "off-hover", label: "Not connected · hover reveals the name", props: { api: apis.off, hover: true } },
    { id: "off-open", label: "Not connected · selling popover", props: { api: apis.off, open: true } },
    { id: "on", label: "Connected", props: { api: apis.on } },
    { id: "on-open", label: "Connected · spend · live · recent reflexes", props: { api: apis.on, open: true } },
    { id: "quiet-open", label: "Connected · no reflexes yet", props: { api: apis.quiet, open: true } },
  ],
});

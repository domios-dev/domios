import type { UsageSnapshot } from "../../../../src/types/usage";
import { CLAUDE, CODEX } from "../../gallery/fixtures";
import { FILM_NOW, HOUR } from "./clock";
import { filmRoute } from "./daemon";
import { MEMBERS } from "./team";

function usage(provider: string, session: number, weekly: number): UsageSnapshot {
  return {
    billing_mode: "subscription", limits_reported: true, id: 1, provider, account_id: null, captured_at: FILM_NOW, parsed_ok: true,
    session_pct: session, session_resets_at: FILM_NOW + 2 * HOUR, weekly_pct: weekly, weekly_resets_at: FILM_NOW + 72 * HOUR,
    spend_cents: null, spend_limit_cents: null, spend_remaining_cents: null, spend_resets_at: null, failure_reason: null, probe_command: null,
  };
}

const USAGE: Record<string, UsageSnapshot> = { claude: usage("claude", 34, 12), codex: usage("codex", 58, 31) };
const IDENTITY = { uid: "film-owner", email: null, display_name: "Sample Account", provider: "github" };

filmRoute("GET", "/api/agents/providers", () => [CLAUDE, CODEX]);
filmRoute("GET", "/api/usage/latest", (url) => USAGE[url.searchParams.get("provider") ?? ""] ?? null);
filmRoute("GET", "/api/agents/providers/claude/model-usage", () => []);
filmRoute("POST", "/api/plugins/bubble", () => ({ label: "release/2.4", tone: "accent", tooltip: "Git · release/2.4 · clean", action: "openPanel:git" }));
filmRoute("POST", "/api/repo-watch/subscribe", (_url, body) => {
  const cwd = String((body as { cwd?: unknown } | undefined)?.cwd ?? "");
  return { status: "watching", token: `watch-${cwd}`, identity: cwd };
});
filmRoute("GET", "/api/plans", () => ({ plans: [] }));
filmRoute("GET", "/api/shells", () => []);
filmRoute("GET", `/api/tabs/${MEMBERS[0].id}/delayed-messages`, () => []);
filmRoute("GET", `/api/tabs/${MEMBERS[0].id}/system-heartbeats`, () => []);
filmRoute("GET", "/api/automation/graph", () => ({ workspace_id: null, team_id: null, all_teams: true, revision: `film-${FILM_NOW}`, scenarios: [], nodes: [], edges: [], blueprints: [] }));
filmRoute("GET", "/api/keepawake", () => ({ active: false, display: false, expires_at: null }));
filmRoute("GET", "/api/account/status", () => ({ identity: IDENTITY, pending: false }));
filmRoute("GET", "/api/account/plan", () => ({
  entitlements: { plan: "pro", status: "active", current_period_end: 2_000_000_000, cancel_at_period_end: false },
  access: { authority: "billing", full_access: false }, stale: false, limits: { enforcing: true, workspaces: null, teams: null, "agent.tabs": null },
}));
filmRoute("GET", "/api/update/channel", () => ({ channel: "stable" }));
filmRoute("GET", "/api/update/availability", () => ({ items: [], errors: {} }));

import type { Plan } from "../../components/Canvas/types";
import type { PlanIndexEntry } from "../../components/Canvas/plansApi";
import type { AutomationGraph, AutomationIdentity } from "../../components/Automation/types";
import type { NodeFile } from "../../components/Automation/nodeFilesModel";
import type { CronJob } from "../types";
import type { TeamMemberAttachment, TeamCloseInventory } from "../teamActions";

export interface OrchestrationClient {
  json<T>(method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE", path: string, body?: unknown, opts?: { signal?: AbortSignal; timeoutMs?: number; callerTabId?: string }): Promise<T>;
}

export interface CallerClaim {
  callerTabId?: string | null;
  sessionBindingNonce?: string;
}
export interface GraphScope {
  workspaceId: string;
  teamId: string | null;
}
export interface ThreadState {
  bridge_active: boolean;
  topics: "available" | "unavailable" | "unknown";
  bound: boolean;
  link: string | null;
}
export interface OpenedThread {
  tab_id: string;
  chat_id: number;
  thread_id: number;
  topic_name: string;
  link: string | null;
  created: boolean;
}

const enc = encodeURIComponent;
function required(value: string, name: string): string {
  if (!value?.trim()) throw new Error(`${name} is required`);
  return value;
}
function scopeQuery(scope: GraphScope): string {
  required(scope.workspaceId, "Automation workspace scope");
  if (scope.teamId !== null && typeof scope.teamId !== "string") throw new Error("Automation Team scope is required");
  return new URLSearchParams({ workspace_id: scope.workspaceId, team_id: scope.teamId ?? "" }).toString();
}
function claimBody(claim: CallerClaim = {}) {
  return { caller_tab_id: claim.callerTabId ?? undefined, session_binding_nonce: claim.sessionBindingNonce };
}
function scopeBody(scope: GraphScope) {
  scopeQuery(scope);
  return { workspace_id: scope.workspaceId, team_id: scope.teamId };
}

export function createOrchestrationTransport(client: OrchestrationClient) {
  const tabPath = (tabId: string) => `/api/tabs/${enc(required(tabId, "Tab identity"))}`;
  const cronPath = (id: string) => `/api/crons/${enc(required(id, "Cron identity"))}`;
  const blueprintPath = (id: string) => `/api/automation/blueprints/${enc(required(id, "Blueprint identity"))}`;
  const teamPath = (teamId: string) => `/api/teams/${enc(required(teamId, "Team identity"))}`;
  return {
    get_plan_full: ({ planId }: { planId: string }, opts?: { signal?: AbortSignal }) => client.json<Plan>("GET", `/api/plans/${enc(required(planId, "Plan identity"))}`, undefined, opts),
    async list_plan_index(opts?: { signal?: AbortSignal }): Promise<PlanIndexEntry[]> {
      return (await client.json<{ plans: PlanIndexEntry[] }>("GET", "/api/plans", undefined, opts)).plans;
    },
    async list_plans_full(args: { project?: string | null } = {}, opts?: { signal?: AbortSignal }): Promise<Plan[]> {
      const query = new URLSearchParams({ full: "1" });
      if (args.project) query.set("project", args.project);
      return (await client.json<{ plans: Plan[] }>("GET", `/api/plans?${query}`, undefined, opts)).plans;
    },
    automation_graph_snapshot: (args: GraphScope) => client.json<AutomationGraph>("GET", `/api/automation/graph?${scopeQuery(args)}`),
    automation_graph_workspace_snapshot: ({ workspaceId }: { workspaceId: string }) => client.json<AutomationGraph>("GET", `/api/automation/graph/workspace?workspace_id=${enc(required(workspaceId, "Automation workspace scope"))}`),
    automation_graph_stop_blueprint: (args: GraphScope & CallerClaim & { id: string }) => client.json<{ id: string; state: string; reason: string }>("POST", `${blueprintPath(args.id)}/stop`, { ...scopeBody(args), ...claimBody(args) }),
    automation_graph_delete_blueprint: (args: GraphScope & { id: string }) => client.json<void>("DELETE", blueprintPath(args.id), scopeBody(args)),
    automation_node_files: (args: { identity: AutomationIdentity; bucket?: string | null; handler?: string | null }) => client.json<NodeFile[]>("POST", "/api/automation/node-files", args),
    blueprint_export_file: (args: GraphScope & { id: string }) => client.json<unknown>("POST", "/api/automation/blueprints/export", { ...scopeBody(args), id: args.id }),
    blueprint_import_file: (args: { callerTabId: string; recipe: unknown; assignments: string[] }) => client.json<unknown>("POST", "/api/automation/blueprints/import", { caller_tab_id: args.callerTabId, recipe: args.recipe, assignments: args.assignments }),
    bridge_thread_state: ({ tabId }: { tabId: string }) => client.json<ThreadState>("GET", `${tabPath(tabId)}/bridge/thread`),
    bridge_thread_open: (args: CallerClaim & { tabId: string }) => client.json<OpenedThread>("POST", `${tabPath(args.tabId)}/bridge/thread`, { selected_tab_id: args.tabId, ...claimBody(args) }),
    bridge_thread_close: (args: CallerClaim & { tabId: string }) => client.json<{ outcomes: unknown[] }>("DELETE", `${tabPath(args.tabId)}/bridge/thread`, { selected_tab_id: args.tabId, ...claimBody(args) }),
    activity_list: () => client.json<unknown>("GET", "/api/activity"),
    cron_list: (args: { workspaceId?: string } = {}) => client.json<CronJob[]>("GET", `/api/crons${args.workspaceId ? `?workspace_id=${enc(args.workspaceId)}` : ""}`),
    cron_get: ({ id }: { id: string }) => client.json<CronJob>("GET", cronPath(id)),
    cron_history: ({ id }: { id: string }) => client.json<unknown[]>("GET", `${cronPath(id)}/history`),
    cron_delete: ({ id }: { id: string }) => client.json<void>("DELETE", cronPath(id)),
    cron_trigger: ({ id }: { id: string }) => client.json<unknown>("POST", `${cronPath(id)}/trigger`),
    cron_cancel: ({ id }: { id: string }) => client.json<unknown>("POST", `${cronPath(id)}/cancel`),
    cron_toggle: ({ id, enabled }: { id: string; enabled: boolean }) => client.json<unknown>("POST", `${cronPath(id)}/${enabled ? "enable" : "disable"}`),
    cron_update: ({ id, params }: { id: string; params: Record<string, unknown> }) => client.json<CronJob>("PATCH", cronPath(id), params),
    mux_rename_team: (args: CallerClaim & { teamId: string; name: string }) => client.json<unknown>("POST", `${teamPath(args.teamId)}/rename`, { name: args.name, ...claimBody(args) }),
    mux_close_team: (args: CallerClaim & { teamId: string; forceBusy?: boolean }) => client.json<{ inventory: TeamCloseInventory }>("POST", `${teamPath(args.teamId)}/close`, { force: true, confirm: args.teamId, force_busy: args.forceBusy === true, ...claimBody(args) }),
    mux_add_team_member: (args: CallerClaim & { teamId: string; provider: string; role: string; model?: string; brief?: string }) => client.json<TeamMemberAttachment>("POST", `${teamPath(args.teamId)}/members`, { provider: args.provider, role: args.role, model: args.model, task: args.brief, ...claimBody(args) }),
    mux_make_orchestrator: ({ tabId }: { tabId: string }) => client.json<unknown>("POST", `${tabPath(tabId)}/make-orchestrator`),
    mux_make_planner: ({ tabId, planId }: { tabId: string; planId: string }) => client.json<unknown>("POST", `${tabPath(tabId)}/make-planner`, { plan_id: planId }),
    mux_create_orchestration_pair: ({ params }: { params: Record<string, unknown> }) => client.json<unknown>("POST", "/api/orchestration/new", params),
  };
}

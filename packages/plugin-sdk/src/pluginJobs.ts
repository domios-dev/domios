import type { BlueprintOperationState } from "./blueprintSteps";
import type { BlueprintStepContext, BlueprintStepEstimate, BlueprintOperationApproval } from "./blueprintSteps";

export interface JobLifecycleDescriptor { contractVersion: 1; reconcile: true; cancel: boolean; }
export interface PluginJobScope { plugin: string; workspace: string; team: string | null; owner: string; }
export type PluginJobOrigin =
  | {kind: "agent_command"; surface: string; context: string; tab: string; registration: string}
  | {kind: "studio_view"; surface: string; instance: string; tab: string; frame: string}
  | {kind: "studio_agent"; surface: string; instance: string; tab: string; actor: string}
  | {kind: "blueprint"; blueprint: string; run: string; node: string; attempt: string};
export interface JobContext {scope: PluginJobScope; origin: PluginJobOrigin; operation: string; job: string; deadlineMs: number;}
export interface JobObservation {job: string; state: "queued" | "running" | "succeeded" | "failed" | "canceled" | "unknown";}
export interface PluginJobMonitor {
  registration: {scope: PluginJobScope; origin: PluginJobOrigin; operation: string; job: string; nextCheckMs: number; deadlineMs: number};
  nextCheckMs: number; state: BlueprintOperationState; checks: number; cancelRequested: boolean; cancelDispatched: boolean;
  stopped: "deadline" | "check_limit" | "owner_stopped" | "scope_removed" | "plugin_unavailable" | null;
}
export interface JobOriginReceipt {operation: string; deadlineMs: number; scope: PluginJobScope; origin: PluginJobOrigin; authorization: "observation_only";}
export interface JobQuote {binding: string; amount: number; unit: string; basis: string; tariffRevision: string; issuedAt: number; validUntilMs: number;}
export interface JobSpendReceipt {operation: string; scope: PluginJobScope; origin: PluginJobOrigin; quote: JobQuote; request: Record<string, unknown>; dailyAllowance: number; state: "waiting_for_owner" | "approved_once" | "approved_budget" | "denied" | "entered_unknown" | "expired_readmit_needed"; enteredAtMs: number | null;}
export type JobTrackResult =
  | {operation: string; job: string; registered: true; monitor: PluginJobMonitor}
  | {operation: string; job: string; registered: false; state: "unknown"; reason: "deadline" | "plugin_unavailable" | "monitor_unavailable"};
export interface JobStatus {operation: string; job: string | null; scope: PluginJobScope; origin: PluginJobOrigin; spend: JobSpendReceipt | null; monitor: PluginJobMonitor | null;}
export interface JobRefusal {error: "plugin_jobs_unavailable" | "plugin_job_request_refused";}
export interface HostJobs {
  operations(): BlueprintOperationView[] | JobRefusal;
  admit(): JobOriginReceipt | JobRefusal;
  requestBinding(request: Record<string, unknown>): {binding: string} | JobRefusal;
  prepare(operation: string, request: Record<string, unknown>, quote: JobQuote): JobSpendReceipt | JobRefusal;
  enter(operation: string, request: Record<string, unknown>): JobSpendReceipt | JobRefusal;
  track(operation: string, job: string): JobTrackResult | JobRefusal;
  status(operation: string): JobStatus | JobRefusal;
  check(operation: string): JobStatus | JobRefusal;
  cancel(operation: string): JobStatus | JobRefusal;
  stop(operation: string): JobStatus | JobRefusal;
}
export interface BlueprintOperationView {
  decisionKey: string;
  cancelRequested: boolean;
  operation: string; context: BlueprintStepContext; state: BlueprintOperationState; job: string | null;
  waiting: {reason: "estimate_missing" | "estimate_stale" | "over_run_ceiling" | "over_step_allowance" | "owner_confirmation"; estimate: number | null; remaining: number; unit: string} | null;
  estimate: BlueprintStepEstimate | null; authorization: BlueprintOperationApproval | null;
}
export interface PluginJobLifecycle {
  jobReconcile(context: JobContext): JobObservation;
  jobCancel?(context: JobContext): JobObservation;
}

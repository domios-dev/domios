import { tabActivity } from "../../lib/tabActivity";
import { tabLaunched } from "../../lib/tabFailure";
import { runtimeResponseDeadlinePending } from "../../lib/agentRuntimeState";
import type { Tab } from "../../lib/types";
import type { ChatBodyView } from "./chatBodyVisibility";

export function chatSetupView(tab: Partial<Tab> | null | undefined, chatSessionAttached = false): ChatBodyView {
  if (!chatSessionAttached && runtimeResponseDeadlinePending(tab?.agent_runtime_state, tab?.boot_failure_reason)) return { kind: "starting", label: "Starting…" };
  const launched = tabLaunched(tab);
  if (tab?.boot_stage === "spawn_failed" && !launched) return { kind: "launch_failed", reason: tab.boot_failure_reason ?? null };
  if (chatSessionAttached) return { kind: "none" };
  const activity = tabActivity(tab);
  // A launched session's failure docks in the status row; the body stays its transcript.
  if (activity.kind === "failed" && launched) return { kind: "none" };
  if (activity.kind === "failed" && !tab?.agent_booting) return { kind: "launch_failed", reason: tab?.boot_failure_reason ?? tab?.agent_runtime_state?.reason ?? null };
  return activity.launching || activity.kind === "failed"
    ? { kind: "starting", label: activity.label }
    : { kind: "none" };
}

export function heldMessagesForLaunch(sends: readonly { dismissed?: boolean; status?: { state: string } }[]): number {
  return sends.filter(send => !send.dismissed && send.status?.state === "held").length;
}

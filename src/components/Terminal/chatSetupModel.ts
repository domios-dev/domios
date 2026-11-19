import { hasExitedChatRecipient, tabActivity } from "../../lib/tabActivity";
import { tabLaunched } from "../../lib/tabFailure";
import { runtimeResponseDeadlinePending } from "../../lib/agentRuntimeState";
import type { ExpectedChatRecipient, Tab } from "../../lib/types";
import type { ChatBodyView } from "./chatBodyVisibility";
import { describeError, type ErrorDescription } from "../../lib/describeError";

export function composerSendPolicy(tab: Partial<Tab> | null | undefined) {
  const exited = hasExitedChatRecipient(tab) ? tab?.exited_chat_recipient : undefined;
  const stopped = tab?.backing?.kind === "pty" && !tab.agent_type && !exited
    && Boolean(tab.prior_session_binding_nonces?.length);
  const recipient: ExpectedChatRecipient | undefined = exited
    ? { provider_id: exited.provider_id, binding_nonce: exited.nonce, session_id: exited.session_id }
    : tab?.agent_type && tab.backing?.kind !== "chat_backend" && (tab.session_binding_nonce || tab.agent_session_id)
      ? { provider_id: tab.agent_type, binding_nonce: tab.session_binding_nonce ?? null, session_id: tab.agent_session_id ?? null }
      : undefined;
  return {
    allowed: Boolean(tab) && !stopped,
    recipient,
  };
}

export function chatSetupView(tab: Partial<Tab> | null | undefined, chatSessionAttached = false): ChatBodyView {
  if (hasExitedChatRecipient(tab) && !tab?.agent_booting) return { kind: "exited" };
  const launched = tabLaunched(tab);
  if (tab?.boot_stage === "spawn_failed" && !launched) return { kind: "launch_failed", reason: tab.boot_failure_reason ?? null };
  const currentLaunchBound = Boolean(tab?.agent_session_id?.trim() && tab.session_binding_nonce
    && tab.reported_session_binding_nonce === tab.session_binding_nonce);
  if (tab?.boot_stage !== "spawn_failed" && tab?.boot_stage !== "awaiting_owner_prompt"
    && tab?.backing?.kind === "pty" && tab.pty_id && !tab.frozen && !currentLaunchBound
    && tab.pending_prompt_kind === "provider" && tab.pending_prompt) return { kind: "terminal_prompt" };
  if (tab?.boot_stage !== "spawn_failed" && !chatSessionAttached && runtimeResponseDeadlinePending(tab?.agent_runtime_state, tab?.boot_failure_reason)) return { kind: "starting", label: "Starting…" };
  if (chatSessionAttached) return { kind: "none" };
  const activity = tabActivity(tab);
  // A launched session's failure docks in the status row; the body stays its transcript.
  if (activity.kind === "failed" && launched) return { kind: "none" };
  if (activity.kind === "failed" && !tab?.agent_booting) return { kind: "launch_failed", reason: tab?.boot_failure_reason ?? tab?.agent_runtime_state?.reason ?? null };
  return activity.launching || activity.kind === "failed"
    ? { kind: "starting", label: activity.label }
    : { kind: "none" };
}

export function chatLaunchFailureDescription(tab: Partial<Tab> | null | undefined, reason: string | null): ErrorDescription | undefined {
  if (tab?.boot_stage !== "spawn_failed" || tab.backing?.kind === "chat_backend") return undefined;
  const description = describeError(reason);
  const prompt = tab.pending_prompt_kind === "provider" ? tab.pending_prompt?.question?.trim() : null;
  return { ...description, pending: false, title: "Launch failed",
    cause: prompt || (description.pending ? "The agent could not finish starting. Open its terminal to check for a prompt or more details." : description.cause) };
}

export function heldMessagesForLaunch(sends: readonly { dismissed?: boolean; status?: { state: string } }[]): number {
  return sends.filter(send => !send.dismissed && send.status?.state === "held").length;
}

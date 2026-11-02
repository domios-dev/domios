import type { AutoConfirmedPrompt, Tab } from "./types";

export function autoConfirmationsForSession(records: AutoConfirmedPrompt[] | undefined, sessionId: string | null): AutoConfirmedPrompt[] {
  if (!records || !sessionId) return [];
  const seen = new Set<string>();
  return records.filter(record => {
    if (record.session_id !== sessionId || seen.has(record.prompt_fingerprint)) return false;
    seen.add(record.prompt_fingerprint);
    return true;
  });
}

export function providerPromptKey(tab: Partial<Tab> | null | undefined): string | null {
  const prompt = tab?.pending_prompt;
  return prompt ? JSON.stringify([tab?.agent_session_id, tab?.session_binding_nonce, tab?.pty_id,
    tab?.pending_prompt_kind, prompt.request_id ?? prompt]) : null;
}

export interface ProviderPromptLifecycle {
  scope: string;
  key: string | null;
  resolved: boolean;
  confirmations: string[];
  activity: Tab["agent_state"];
}

export function advanceProviderPrompt(previous: ProviderPromptLifecycle | null, tab: Partial<Tab> | null | undefined,
  answeredKey: string | null = null): ProviderPromptLifecycle {
  const scope = JSON.stringify([tab?.id, tab?.agent_session_id, tab?.session_binding_nonce, tab?.pty_id]);
  const key = providerPromptKey(tab);
  const records = autoConfirmationsForSession(tab?.permission_auto_confirmations, tab?.agent_session_id ?? null);
  const sameScope = previous?.scope === scope;
  const samePrompt = sameScope && previous?.key === key;
  const prompt = tab?.pending_prompt;
  const confirmed = samePrompt && prompt && records.some(record => !previous.confirmations.includes(record.prompt_fingerprint)
    && record.question === prompt.question && record.options.length === prompt.options.length
    && record.options.every((label, index) => label === prompt.options[index].label));
  const resumed = samePrompt && previous.activity?.state === "question"
    && (tab?.agent_state?.state === "working" || tab?.agent_state?.state === "idle");
  return { scope, key, resolved: Boolean(key && (answeredKey === key || confirmed || resumed || (samePrompt && previous.resolved))),
    confirmations: records.map(record => record.prompt_fingerprint), activity: tab?.agent_state };
}

export function providerPromptPresentation<T extends Partial<Tab>>(tab: T | null | undefined, lifecycle: ProviderPromptLifecycle): T | null | undefined {
  if (!tab || !lifecycle.resolved) return tab;
  return { ...tab, pending_prompt: null, pending_prompt_kind: null,
    agent_state: tab.agent_state?.state === "question" ? { state: "idle" } : tab.agent_state };
}

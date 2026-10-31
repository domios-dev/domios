import type { NotificationPromptIdentity, PendingPrompt, PromptOption, Tab } from "./types";

export function manualPendingPrompt(tab: Pick<Tab, "pending_prompt" | "pending_prompt_kind" | "auto_approve" | "permission_auto_confirm" | "remote_peer_id"> | null | undefined): PendingPrompt | null {
  return !tab?.remote_peer_id && tab?.pending_prompt_kind === "automatic_permission" && (tab.auto_approve || tab.permission_auto_confirm)
    ? null : tab?.pending_prompt ?? null;
}

export function answerablePrompt<T>(
  descriptor: { supports_prompt_answers?: boolean } | null | undefined,
  prompt: T | null,
): T | null {
  return descriptor?.supports_prompt_answers === true ? prompt : null;
}

export function promptMessagePart(
  descriptor: { supports_prompt_answers?: boolean; supports_structured_prompts?: boolean } | null | undefined,
  prompt: PendingPrompt | null,
  answeredQuestion: string | null = null,
) {
  const pending = descriptor?.supports_structured_prompts || prompt?.question !== answeredQuestion ? prompt : null;
  const answerable = answerablePrompt(descriptor, pending);
  return answerable ? { kind: "prompt" as const, id: "pending-prompt:" + JSON.stringify(answerable), prompt: answerable } : null;
}

export function isFreeTextPromptOption(option: PromptOption): boolean {
  return /^(?:\(?\s*)?(?:other|custom|none of (?:the above|these)|type (?:your own|something(?: else)?)|write your own|enter (?:your own|custom|text)|free[\s-]?text|something else)/i.test(option.label.trim());
}

export function promptResponseArgs(tabId: string, value: string, prompt: PendingPrompt | null, kind: NotificationPromptIdentity["kind"] = "structured") {
  return {
    tabId, value,
    ...(prompt?.request_id != null ? {
      identity: {
        kind,
        options: prompt.options.map((option) => ({ ...option })),
        request_id: prompt.request_id,
      },
    } : {}),
  };
}

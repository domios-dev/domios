import { expect, it } from "vitest";
import type { AutoConfirmedPrompt, Tab } from "./types";
import { advanceProviderPrompt, autoConfirmationsForSession, providerPromptKey, providerPromptPresentation } from "./providerPromptLifecycle";

const tab: Partial<Tab> = { id: "tab", agent_session_id: "session", session_binding_nonce: "launch", pty_id: "pty",
  pending_prompt_kind: "provider", agent_state: { state: "question" },
  pending_prompt: { request_id: "request", question: "opaque-question", options: [{ label: "opaque-choice", value: "1" }] } };
const confirmation: AutoConfirmedPrompt = { session_id: "session", prompt_fingerprint: "fingerprint", tool_name: "opaque-tool",
  question: "opaque-question", options: ["opaque-choice"], option_label: "opaque-choice", rule: "opaque-rule", confirmed_at_ms: 1 };

it("keeps a pending prompt waiting without resolution evidence", () => {
  const state = advanceProviderPrompt(null, tab);
  expect(state.resolved).toBe(false);
  expect(providerPromptPresentation(tab, state)).toBe(tab);
  expect(advanceProviderPrompt(state, tab).resolved).toBe(false);
});

it("resolves from a newly recorded own-session auto-confirm and remains resolved", () => {
  const before = advanceProviderPrompt(null, tab);
  const confirmed = { ...tab, permission_auto_confirmations: [confirmation] };
  const state = advanceProviderPrompt(before, confirmed);
  expect(state.resolved).toBe(true);
  expect(providerPromptPresentation(confirmed, state)).toMatchObject({ pending_prompt: null, agent_state: { state: "idle" } });
  expect(advanceProviderPrompt(state, confirmed).resolved).toBe(true);
});

it("resolves a successful manual answer only for its captured identity", () => {
  const before = advanceProviderPrompt(null, tab);
  expect(advanceProviderPrompt(before, tab, providerPromptKey(tab)).resolved).toBe(true);
  const next = { ...tab, pending_prompt: { ...tab.pending_prompt!, request_id: "next" } };
  expect(advanceProviderPrompt(before, next, providerPromptKey(tab)).resolved).toBe(false);
});

it.each(["working", "idle"] as const)("resolves when the same prompt's question transitions to %s", state => {
  const before = advanceProviderPrompt(null, tab);
  expect(advanceProviderPrompt(before, { ...tab, agent_state: { state } }).resolved).toBe(true);
  expect(advanceProviderPrompt(null, { ...tab, agent_state: { state } }).resolved).toBe(false);
});

it("preserves a superseding prompt and a new launch despite old answers or activity", () => {
  const before = advanceProviderPrompt(null, tab);
  const resolved = advanceProviderPrompt(before, tab, providerPromptKey(tab));
  const next = { ...tab, pending_prompt: { ...tab.pending_prompt!, request_id: "next" }, agent_state: { state: "working" as const } };
  expect(advanceProviderPrompt(resolved, next).resolved).toBe(false);
  expect(advanceProviderPrompt(resolved, { ...tab, session_binding_nonce: "next-launch" }).resolved).toBe(false);
  expect(advanceProviderPrompt(resolved, { ...tab, pending_prompt: null }).key).toBeNull();
});

it("ignores historical, foreign and different auto-confirmations", () => {
  const history = { ...tab, permission_auto_confirmations: [confirmation] };
  const before = advanceProviderPrompt(null, history);
  expect(advanceProviderPrompt(before, history).resolved).toBe(false);
  for (const record of [{ ...confirmation, session_id: "foreign" }, { ...confirmation, question: "different" },
    { ...confirmation, options: ["different"] }]) {
    expect(advanceProviderPrompt(advanceProviderPrompt(null, tab), { ...tab, permission_auto_confirmations: [record] }).resolved).toBe(false);
  }
});

it("projects each confirmation once for the current session", () => {
  expect(autoConfirmationsForSession([confirmation, confirmation, { ...confirmation, session_id: "foreign" }], "session")).toEqual([confirmation]);
});

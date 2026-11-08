import { renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { kind, textContent } from "../../restart-endurance/oracle.mjs";
import { marker } from "./fixtures.mjs";
import { dcJson } from "./env.mjs";
import { preparePolicy } from "./model-policy.mjs";
import { chatMessages, normalizeAnswer, replyVerdict } from "./oracle.mjs";
import { readTab, readTabs, sendText, withEvents } from "./tabs.mjs";
import { observe, touchesTab } from "./wait.mjs";

const tabEvidence = tab => ({
  id: tab.id, session: tab.agent_session_id, pty_id: tab.pty_id ?? null,
  boot_stage: tab.boot_stage, binding_current: tab.reported_session_binding_nonce === tab.session_binding_nonce,
});

export async function recordChatActivationObservation(ctx) {
  const record = { run: ctx.state.run, started_at: new Date().toISOString(), pre_spawn: false, delivered_once: null, pty_started: null };
  try {
    const owned = new Set(ctx.state.tabs.filter(tab => tab.provider === "claude" && tab.journey === "J2").map(tab => tab.id));
    const candidate = (await readTabs(ctx.api)).find(tab => owned.has(tab.id) && tab.boot_stage === "pty_starting" && tab.backing?.kind === "pty");
    if (!candidate) record.unavailable = "No owned lazy Claude PTY was available before J4 activation";
    else {
      record.before = tabEvidence(candidate);
      const observationCtx = { ...ctx, state: structuredClone(ctx.state), save: () => {} };
      observationCtx.policy = await preparePolicy(observationCtx, { readRecord: (id, signal) => readTab(ctx.api, id, signal) });
      await withEvents(observationCtx, ctx.deadlines.reply, async (events, signal) => {
        const current = await readTab(ctx.api, candidate.id, signal);
        record.before = tabEvidence(current);
        if (current.boot_stage !== "pty_starting") {
          record.unavailable = "The candidate activated before the observation send";
          return;
        }
        const baseline = chatMessages(await ctx.api(`/api/tabs/${candidate.id}/chat`, { signal }));
        const baselineIds = new Set(baseline.map(message => message.id));
        const expected = marker(ctx.state.run, candidate.id, "chat-activation-observation");
        const prompt = `Reply with exactly ${expected} as one contiguous string and nothing else. Do not run tools.`;
        record.send_call_started = true;
        record.receipt = await sendText(observationCtx, candidate.id, prompt);
        const outcome = await observe({
          events, filter: touchesTab(candidate.id), label: "non-gating lazy Chat activation observation",
          read: async () => {
            const [tab, chat] = await Promise.all([
              readTab(ctx.api, candidate.id, signal), ctx.api(`/api/tabs/${candidate.id}/chat`, { signal }),
            ]);
            return { tab, messages: chatMessages(chat) };
          },
          judge: ({ tab, messages }) => {
            const verdict = replyVerdict({ record: tab, messages, baselineIds, marker: expected, boundSession: candidate.agent_session_id });
            return verdict.done ? { done: { tab, messages } } : verdict;
          },
        });
        record.after = tabEvidence(outcome.tab);
        record.session_preserved = outcome.tab.agent_session_id === candidate.agent_session_id;
        record.pty_started = Boolean(outcome.tab.pty_id && outcome.tab.pty_id !== record.before.pty_id
          && ["agent_ready", "pty_reattached"].includes(outcome.tab.boot_stage) && record.after.binding_current);
        const appended = outcome.messages.filter(message => !baselineIds.has(message.id));
        record.user_turn_ids = appended.filter(message => kind(message) === "user").map(message => message.id);
        record.exact_user_turn_ids = appended.filter(message => kind(message) === "user" && textContent(message.content).trim() === prompt).map(message => message.id);
        record.assistant_reply_ids = appended.filter(message => kind(message) === "assistant" && normalizeAnswer(textContent(message.content)) === expected).map(message => message.id);
        record.inspection = await dcJson(["tab", "send-status", record.receipt.send_id], { timeoutMs: 30_000 });
        if (record.inspection.state === "delivered") record.delivered_once = record.session_preserved && record.user_turn_ids.length === 1 && record.assistant_reply_ids.length === 1;
      });
    }
  } catch (error) {
    record.error = String(error.message).split(ctx.dev.token).join("<redacted>").slice(0, 1000);
    try {
      if (record.before?.id) record.after = tabEvidence(await readTab(ctx.api, record.before.id));
      if (record.receipt && !record.inspection) record.inspection = await dcJson(["tab", "send-status", record.receipt.send_id], { timeoutMs: 30_000 });
    } catch (inspectionError) {
      record.inspection_error = String(inspectionError.message).split(ctx.dev.token).join("<redacted>").slice(0, 1000);
    }
  }
  record.finished_at = new Date().toISOString();
  const path = join(ctx.out, "chat-activation-observation.json");
  try {
    writeFileSync(`${path}.next`, `${JSON.stringify(record, null, 2)}\n`);
    renameSync(`${path}.next`, path);
  } catch (error) {
    console.log(`OBSERVATION storage unavailable: ${error.code ?? "unknown"}`);
  }
  console.log(`OBSERVATION normal_chat_lazy_pty tab=${record.before?.id ?? "none"} pty_started=${record.pty_started} delivered_once=${record.delivered_once}`);
}

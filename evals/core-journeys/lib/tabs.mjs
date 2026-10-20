import { retainedSendReceipt } from "../../restart-endurance/send-receipt.mjs";
import { Events, dc, dcJson } from "./env.mjs";
import { chatMessages, readiness, replyVerdict, stopVerdict } from "./oracle.mjs";
import { JourneyFailure, observe, touchesTab } from "./wait.mjs";

export async function readTabs(api, signal) {
  const [projection, mux] = await Promise.all([
    api("/api/tabs?local_only=true&all_workspaces=true", { signal }),
    api("/api/mux/state", { signal }),
  ]);
  const full = Object.fromEntries(Object.values(mux?.tabs ?? {}).map((record) => [record.id, record]));
  const rows = Array.isArray(projection) ? projection : projection?.tabs ?? [];
  return rows.map((summary) => ({ ...full[summary.id], ...summary, status: summary.status }));
}

export async function readTab(api, id, signal) {
  return (await readTabs(api, signal)).find((record) => record.id === id) ?? null;
}

export async function withEvents(ctx, deadlineMs, body) {
  const signal = AbortSignal.timeout(deadlineMs);
  const events = await new Events(ctx.dev, signal).open();
  try { return await body(events, signal); } finally { events.close(); }
}

export function waitReady(ctx, id, { requireSession = false, deadlineMs = ctx.deadlines.ready } = {}) {
  return withEvents(ctx, deadlineMs, (events, signal) => observe({
    events, filter: touchesTab(id), label: `tab ${id} ready`,
    read: () => readTab(ctx.api, id, signal),
    judge: (record) => readiness(record, { requireSession }),
  }));
}

export async function sendText(id, text) {
  const result = await dc(["send", "--tab", id, "--require-agent", "--stdin", "--json"], { stdin: text });
  if (result.code === 0 || retainedSendReceipt(result.code, result.stdout)) return result.stdout;
  throw new JourneyFailure(`send refused (exit ${result.code}): ${(result.stdout || result.stderr).slice(0, 600)}`);
}

async function chat(ctx, id, signal) {
  return chatMessages(await ctx.api(`/api/tabs/${id}/chat`, { signal }));
}

// The baseline is taken under the open subscription, so no reply can land between read and send.
export function probeReply(ctx, id, marker, prompt, { deadlineMs = ctx.deadlines.reply } = {}) {
  return withEvents(ctx, deadlineMs, async (events, signal) => {
    const ready = await observe({
      events, filter: touchesTab(id), label: `tab ${id} input ready`,
      read: () => readTab(ctx.api, id, signal), judge: (record) => readiness(record),
    });
    const baselineIds = new Set((await chat(ctx, id, signal)).map((message) => message.id));
    await sendText(id, prompt ?? `Reply with exactly ${marker} and nothing else. Do not run tools.`);
    let boundSession = ready.agent_session_id ?? null;
    const outcome = await observe({
      events, filter: touchesTab(id), label: `tab ${id} reply ${marker}`,
      read: async () => ({ record: await readTab(ctx.api, id, signal), messages: await chat(ctx, id, signal) }),
      judge: ({ record, messages }) => {
        boundSession ??= record?.agent_session_id ?? null;
        return replyVerdict({ messages, baselineIds, marker, record, boundSession });
      },
    });
    return { ...outcome, session_before: ready.agent_session_id ?? null };
  });
}

export function stopLongTurn(ctx, id, { deadlineMs = ctx.deadlines.reply } = {}) {
  return withEvents(ctx, deadlineMs, async (events, signal) => {
    await sendText(id, "Count from 1 to 400, one number per line, with no other text. Do not run tools.");
    await observe({
      events, filter: touchesTab(id), label: `tab ${id} working`,
      read: () => readTab(ctx.api, id, signal),
      judge: (record) => {
        if (record?.status === "working") return { done: record };
        const verdict = readiness(record);
        return verdict.fail ? verdict : { pending: `working (status=${record?.status})` };
      },
    });
    const response = await dcJson(["tab", "stop", "--tab", id]);
    return observe({
      events, filter: touchesTab(id), label: `tab ${id} idle after stop`,
      read: () => readTab(ctx.api, id, signal), judge: (record) => stopVerdict(response, record),
    });
  });
}

export async function closeTab(ctx, id) {
  const result = await dc(["tab", "close", "--tab", id]);
  if (result.code !== 0) throw new JourneyFailure(`tab close exit ${result.code}: ${result.stdout || result.stderr}`);
  return withEvents(ctx, ctx.deadlines.ready, (events, signal) => observe({
    events, filter: touchesTab(id), label: `tab ${id} absent`,
    read: () => readTab(ctx.api, id, signal),
    judge: (record) => record ? { pending: "tab absent" } : { done: true },
  }));
}

export function inspectEvidence(ctx, id) {
  return dcJson(["tab", "inspect", "--tab", id, "--lifecycle"]).catch((error) => ({ error: error.message }));
}

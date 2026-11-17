import { readFileSync } from "node:fs";
import { marker } from "../lib/fixtures.mjs";
import { neighborPeer } from "../lib/neighbor-peer.mjs";
import { readiness } from "../lib/oracle.mjs";
import { readTab, withEvents } from "../lib/tabs.mjs";
import { JourneyFailure, observe } from "../lib/wait.mjs";

export const ACCESS_LEVELS = ["read_only", "folder", "full"];
const runtime = (peer, command, signal) => peer.api("/api/account/neighbors/runtime", { method: "POST", body: command, signal });
const history = (peer, friendship, signal) => runtime(peer, { operation: "history", friendship_id: friendship }, signal);
const demand = (condition, reason) => { if (!condition) throw new JourneyFailure(reason); };
const entryEvidence = entry => entry && Object.fromEntries(["message_id", "direction", "kind", "in_reply_to", "submission", "delivery_status", "delivered_at", "handling_state", "handling_reason", "outcome"].filter(key => entry[key] !== undefined).map(key => [key, entry[key]]));

async function qualify(peer, config, { editsGrant = false } = {}) {
  const [health, backend, account, status, listed, page] = await Promise.all([
    peer.api("/api/health"), peer.api("/api/account/backend"), peer.api("/api/account/status"),
    runtime(peer, { operation: "status" }), peer.api("/api/account/neighbors", { method: "POST", body: { operation: "list" } }),
    history(peer, config.friendship),
  ]);
  demand(health.status === "ok" && health.build_sha === config.candidate, "J5 peer is not on the pinned candidate");
  demand(health.instance?.name === peer.dev.instance && Number(health.instance?.api_port) === peer.dev.port, "J5 health instance mismatch");
  demand(backend.target === "qa" && backend.project_id === "domios-signin-qa", "J5 requires the domios-signin-qa backend");
  demand(account.provider === "firebase" && account.identity?.uid && !account.pending && !account.session_revoked && !account.target_mismatch, "J5 requires a signed-in QA account");
  const friend = listed.friends?.find(row => row.friendship_id === config.friendship);
  demand(friend && listed.access === "paid" && !friend.my_paused && !friend.their_paused && !page.ended, "J5 requires an active, unlocked QA friendship");
  demand(status.enabled && status.friend_receivers?.[config.friendship]?.receiving_here, "J5 peer must already receive this QA friend here");
  demand(status.listeners?.[config.friendship] === "receiving", "J5 requires a healthy receiving listener");
  if (editsGrant) {
    const gateway = status.bindings?.[config.friendship];
    const tab = gateway ? await readTab(peer.api, gateway) : null;
    demand(tab && (tab.agent_state?.state ?? tab.status) === "idle", "J5 requires B's existing QA concierge to be idle before grant edits");
    demand(!(status.incoming_handling?.[config.friendship] ?? []).some(row => ["waiting", "working", "delivered_to_concierge"].includes(row.handling_state)), "J5 pending inbound work must be reconciled before grant edits");
  }
  return { uid: account.identity.uid, friendUid: friend.uid, evidence: {
    instance: peer.dev.instance, port: peer.dev.port, build_sha: health.build_sha, boot_id: health.boot_id, platform: health.platform,
    backend: { target: backend.target, project_id: backend.project_id }, signed_in: true,
  } };
}

async function concierge(peer, friendship, access, ctx) {
  return withEvents(peer, ctx.deadlines.ready, (events, signal) => observe({
    events, label: `J5 ${access} host concierge ready`,
    read: async () => {
      const status = await runtime(peer, { operation: "status" }, signal);
      const id = status.bindings?.[friendship];
      const permission = await runtime(peer, { operation: "permission", friendship_id: friendship }, signal);
      return { id, permission, concierge: status.concierges?.[friendship], tab: id ? await readTab(peer.api, id, signal) : null };
    },
    judge: snapshot => {
      if (snapshot.permission.machine_access !== access) return { fail: "J5 saved machine access differs from requested access" };
      if (snapshot.concierge?.state?.startsWith("concierge_") && snapshot.concierge.state !== "concierge_starting") return { fail: `J5 ${snapshot.concierge.state}` };
      if (!snapshot.id || !snapshot.tab || snapshot.concierge?.state !== "ready") return { pending: "host-created concierge binding" };
      const ready = readiness(snapshot.tab, { requireSession: true });
      if (ready.fail) return ready;
      return ready.done ? { done: { tab: snapshot.id, session: snapshot.tab.agent_session_id, provider: snapshot.tab.agent_type,
        requested_model: snapshot.tab.agent_requested_model, actual_model: snapshot.tab.agent_session_model,
        machine_access: snapshot.permission.machine_access, enforcement: snapshot.permission.folder_enforcement,
        initial_context_digest: snapshot.permission.initial_context_digest } } : ready;
    },
  }));
}

async function roundtrip(ctx, a, b, config, access, row) {
  const friendship = config.friendship;
  const nonce = marker(ctx.state.run, "neighbor", access);
  const expected = config.expected_answer ?? nonce;
  const body = config.question?.replaceAll("{nonce}", nonce)
    ?? `Public synthetic release question: what is the verification code ${expected}? Answer with exactly that code through the trusted Neighbor reply path, linked to this question. Do not answer only in your local chat.`;
  return withEvents(b, ctx.deadlines.reply, async (eventsB, signalB) => withEvents(a, ctx.deadlines.reply, async (eventsA, signalA) => {
    row.send_started_at = new Date().toISOString();
    ctx.save();
    const receipt = await runtime(a, { operation: "send", friendship_id: friendship, kind: "question", body, in_reply_to: null });
    row.question = { message_id: receipt.message_id, status: receipt.status, submission: receipt.submission ?? "accepted" };
    ctx.save();
    demand(receipt.message_id && receipt.status !== "not_delivered" && receipt.submission !== "unconfirmed", "J5 question refused or unconfirmed; never replay");
    const observed = await observe({
      events: eventsB, label: `J5 ${access} linked concierge answer`,
      read: async () => {
        const page = await history(b, friendship, signalB);
        const question = page.entries.find(entry => entry.message_id === receipt.message_id && entry.direction === "in");
        const answers = page.entries.filter(entry => entry.direction === "out" && entry.in_reply_to === receipt.message_id);
        const tab = await readTab(b.api, row.concierge.tab, signalB);
        return { question: entryEvidence(question), answers: answers.map(entryEvidence),
          answer_matches: answers.length === 1 && answers[0].body.trim() === expected,
          answer_by_concierge: answers.length === 1 && answers[0].author?.agent === row.concierge.tab,
          question_matches: question?.body === body, session: tab?.agent_session_id,
          status: tab?.agent_state?.state ?? tab?.status };
      },
      judge: snapshot => {
        if (snapshot.session !== row.concierge.session) return { fail: "J5 concierge session changed during the question" };
        if (snapshot.answers.length > 1) return { fail: "J5 multiple linked replies" };
        const reply = snapshot.answers[0];
        if (reply && (reply.kind !== "answer" || !snapshot.answer_matches || !snapshot.answer_by_concierge)) return { fail: "J5 linked reply is not the expected concierge answer" };
        if (reply?.submission === "unconfirmed" || reply?.delivery_status === "not_delivered") return { fail: "J5 answer unconfirmed or not delivered; never replay" };
        if (reply && snapshot.question_matches && snapshot.question?.handling_state === "answered") return { done: snapshot };
        if (["failed", "stopped", "refused", "no_reply"].includes(snapshot.question?.handling_state)) return { fail: `J5 handling ${snapshot.question.handling_state}: ${snapshot.question.handling_reason ?? "no linked answer"}` };
        return { pending: `linked answer (handling=${snapshot.question?.handling_state ?? "not admitted"})` };
      },
    });
    row.recipient = observed;
    ctx.save();
    row.friend = await observe({
      events: eventsA, label: `J5 ${access} answer arrives at friend`,
      read: async () => {
        const page = await history(a, friendship, signalA);
        const question = page.entries.find(entry => entry.direction === "out" && entry.message_id === receipt.message_id);
        const answers = page.entries.filter(entry => entry.direction === "in" && entry.in_reply_to === receipt.message_id);
        return { question: entryEvidence(question), answers: answers.map(entryEvidence),
          answer_matches: answers.length === 1 && answers[0].body.trim() === expected };
      },
      judge: snapshot => {
        if (snapshot.answers.length > 1) return { fail: "J5 duplicate answer at friend" };
        if (snapshot.answers.length && (!snapshot.answer_matches || snapshot.answers[0].message_id !== observed.answers[0].message_id)) return { fail: "J5 friend received a different answer" };
        return snapshot.answer_matches && snapshot.question?.delivery_status === "delivered" ? { done: snapshot } : { pending: "friend answer and delivered question receipt" };
      },
    });
    row.idle = await observe({
      events: eventsB, label: `J5 ${access} delivered reply and idle concierge`,
      read: async () => {
        const page = await history(b, friendship, signalB);
        const answer = page.entries.find(entry => entry.message_id === observed.answers[0].message_id);
        const tab = await readTab(b.api, row.concierge.tab, signalB);
        return { answer: entryEvidence(answer), session: tab?.agent_session_id, status: tab?.agent_state?.state ?? tab?.status };
      },
      judge: snapshot => snapshot.session !== row.concierge.session ? { fail: "J5 concierge binding changed before idle" }
        : snapshot.answer?.delivery_status === "not_delivered" ? { fail: "J5 answer delivery failed" }
        : snapshot.answer?.delivery_status === "delivered" && snapshot.status === "idle" ? { done: snapshot }
        : { pending: `answer=${snapshot.answer?.delivery_status}, concierge=${snapshot.status}` },
    });
    ctx.save();
    return { summary: `${access}: question and linked concierge answer delivered; same session idle`, evidence: row };
  }));
}

export async function journeyFive(ctx) {
  demand(!ctx.state.j5, "J5 already ran in this output directory; retain its sends and use a fresh --out for a new question");
  let a, b, config, saved;
  ctx.state.j5 = { rows: [], ui: "NOT RUN: verify the friend page through Browser for each access level", attachment: "NOT RUN: optional until reply CLI attachment support lands" };
  const state = ctx.state.j5;
  const qualified = await ctx.check("SETUP", "neighbors_qa_peers", "J5", async () => {
    demand(ctx.options.neighborsConfig, "J5 requires --neighbors-config with qualified disposable QA peers");
    config = JSON.parse(readFileSync(ctx.options.neighborsConfig, "utf8"));
    demand((config.question === undefined && config.expected_answer === undefined)
      || (typeof config.question === "string" && config.question.includes("{nonce}") && typeof config.expected_answer === "string" && config.expected_answer.trim().length > 0), "J5 custom question needs a {nonce} and a nonempty expected_answer matching the public grant");
    demand(config.access_levels === undefined || (Array.isArray(config.access_levels) && config.access_levels.length > 0
      && config.access_levels.every(access => ACCESS_LEVELS.includes(access)) && new Set(config.access_levels).size === config.access_levels.length), "J5 access_levels must be a nonempty subset of read_only, folder and full");
    demand(/^[0-9a-f]{40}$/.test(config.candidate ?? "") && /^[0-9a-f]{64}$/.test(config.friendship ?? ""), "J5 requires a full candidate SHA and friendship ID");
    demand(config.qa_accounts_confirmed === true, "J5 requires private confirmation of the authorized QA account aliases");
    a = { dev: ctx.dev, api: ctx.api };
    b = await neighborPeer(config.peer);
    demand(a.dev.instance !== b.dev.instance && a.dev.port !== b.dev.port, "J5 requires two distinct named peers");
    const qaA = await qualify(a, config), qaB = await qualify(b, config, { editsGrant: true });
    demand(qaA.uid !== qaB.uid && qaA.friendUid === qaB.uid && qaB.friendUid === qaA.uid, "J5 account/friendship identities do not match the QA pair");
    state.peers = [qaA.evidence, qaB.evidence];
    state.friendship = config.friendship;
    saved = await runtime(b, { operation: "permission", friendship_id: config.friendship });
    demand(saved.allowed && saved.scope.trim() && saved.effective_folder, "J5 requires an existing scoped stored grant and concierge folder");
    state.original_access = saved.machine_access;
    ctx.save();
    return { summary: "two distinct signed-in QA peers, same candidate and reciprocal friendship", evidence: state.peers };
  });
  if (!qualified) {
    await ctx.check("J5", "neighbors_roundtrip", "prerequisites", async () => { throw new JourneyFailure("J5 not run: QA peer qualification failed"); });
    return;
  }
  let changed = false;
  try {
    for (const access of config.access_levels ?? ACCESS_LEVELS) {
      const row = { access };
      state.rows.push(row);
      const ready = await ctx.check("J5", "host_concierge_ready", access, async () => {
        changed = true;
        await runtime(b, { operation: "set_permission", friendship_id: config.friendship,
          permission: { machine_access: access, folder: saved.effective_folder } });
        row.concierge = await concierge(b, config.friendship, access, ctx);
        ctx.save();
        return { summary: `${access}: host-created concierge ready from stored grant`, evidence: row.concierge };
      });
      if (!ready) break;
      const result = await ctx.check("J5", "neighbors_roundtrip", access, () => roundtrip(ctx, a, b, config, access, row));
      if (!result) break;
    }
  } finally {
    if (changed) await ctx.check("J5", "permission_restored", "B", async () => {
      const status = await runtime(b, { operation: "status" });
      const tab = await readTab(b.api, status.bindings?.[config.friendship]);
      demand(tab && (tab.agent_state?.state ?? tab.status) === "idle", "J5 leaves the current grant intact while the concierge is active or uncertain; reconcile pending IDs before restoring");
      await runtime(b, { operation: "set_permission", friendship_id: config.friendship,
        permission: { machine_access: saved.machine_access, folder: saved.folder } });
      const actual = await runtime(b, { operation: "permission", friendship_id: config.friendship });
      demand(actual.machine_access === saved.machine_access && actual.folder === saved.folder && actual.scope === saved.scope, "J5 original permission did not persist");
      state.restored = true;
      return "original machine access, folder and scope restored";
    });
  }
}

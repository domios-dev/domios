import { restartObserved } from "../../restart-endurance/restart.mjs";
import { listenerIdentity } from "../lib/listener.mjs";
import { marker } from "../lib/fixtures.mjs";
import { probeReply, readTabs, waitReady } from "../lib/tabs.mjs";
import { JourneyFailure } from "../lib/wait.mjs";

export async function snapshotBeforeRestart(ctx) {
  const owned = new Set(ctx.state.tabs.map((tab) => tab.id));
  const tabs = await readTabs(ctx.api);
  ctx.state.before_restart = {
    listener: listenerIdentity(ctx.dev.port),
    build_sha: ctx.health.build_sha ?? ctx.health.build,
    tabs: tabs.filter((tab) => tab.agent_type && (ctx.options.allTabs || owned.has(tab.id))).map((tab) => ({
      id: tab.id, title: tab.title, agent_type: tab.agent_type, session: tab.agent_session_id ?? null,
      role: tab.role ?? null, owned: owned.has(tab.id), backing: tab.backing?.kind ?? null,
      attempt_id: tab.task_binding?.attempt_id ?? null,
    })),
  };
  ctx.save();
}

export function restartCheck(ctx) {
  return ctx.check("J4", "restart_observed", `${ctx.dev.instance || "default"}:${ctx.dev.port}`, async () => {
    const before = ctx.state.before_restart?.listener;
    if (!before) throw new JourneyFailure("setup recorded no pre-restart listener identity");
    const after = listenerIdentity(ctx.dev.port);
    if (!restartObserved(before, after)) {
      throw new JourneyFailure(`restart_not_observed: listener ${before.pid}@${before.started_us} -> ${after.pid}@${after.started_us}`);
    }
    const build = ctx.health.build_sha ?? ctx.health.build;
    if (ctx.options.expectBuildChange && build === ctx.state.before_restart.build_sha) {
      throw new JourneyFailure(`upgrade did not change the build: still ${build}`);
    }
    return { summary: `pid ${before.pid} -> ${after.pid}, build ${ctx.state.before_restart.build_sha} -> ${ctx.health.build_sha ?? ctx.health.build}`,
      evidence: { before, after } };
  });
}

export async function restoredTabsReady(ctx) {
  const tabs = ctx.state.before_restart?.tabs ?? [];
  await Promise.all(tabs.map((tab) => ctx.check("J4", "restored_tab_ready", `${tab.id} ${tab.agent_type}${tab.role ? `/${tab.role}` : ""}`, async () => {
    const record = await waitReady(ctx, tab.id, { requireSession: Boolean(tab.session) });
    if (tab.session && record.agent_session_id !== tab.session) {
      throw new JourneyFailure(`session changed across restart: ${tab.session} -> ${record.agent_session_id}`);
    }
    if (tab.backing && record.backing?.kind !== tab.backing) {
      throw new JourneyFailure(`backing changed across restart: ${tab.backing} -> ${record.backing?.kind}`);
    }
    if (tab.attempt_id && record.task_binding?.attempt_id !== tab.attempt_id) {
      throw new JourneyFailure(`attempt_id lost across restart: ${tab.attempt_id} -> ${record.task_binding?.attempt_id ?? "none"}`);
    }
    return { summary: `${record.boot_stage} ${record.agent_session_id ?? ""}`.trim(), evidence: { boot_stage: record.boot_stage } };
  })));
}

export async function restoredSends(ctx) {
  const sessions = new Map((ctx.state.before_restart?.tabs ?? []).map((tab) => [tab.id, tab.session]));
  await Promise.all(ctx.state.tabs.filter((tab) => tab.reply).map((tab) =>
    ctx.check("J4", "restored_send_reply", `${tab.id} ${tab.provider}${tab.role ? `/${tab.role}` : ""}`, async () => {
      const expected = sessions.get(tab.id);
      const reply = await probeReply(ctx, tab.id, marker(ctx.state.run, `${tab.provider}${tab.role ?? ""}`, "r"));
      if (expected && reply.session !== expected) throw new JourneyFailure(`reply on session ${reply.session}, expected restored ${expected}`);
      return { summary: `${reply.answer} on ${reply.session}`, evidence: reply };
    })));
}

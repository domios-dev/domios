import { dcJson } from "../lib/env.mjs";
import { historyFiller, marker } from "../lib/fixtures.mjs";
import { selectProviders } from "../lib/oracle.mjs";
import { probeReply, stopLongTurn, waitReady } from "../lib/tabs.mjs";
import { JourneyFailure } from "../lib/wait.mjs";

export async function providers(ctx) {
  const descriptors = await ctx.api("/api/agents/providers");
  return selectProviders(descriptors, { only: ctx.options.providers });
}

async function spawnOne(ctx, provider) {
  const { workspace } = ctx.state;
  const spawned = await dcJson(["agent", "spawn", provider.id, "--workspace", workspace.id, "--cwd", workspace.cwd,
    "--title", `cj${provider.id}`, "--non-user-facing", "--description", `core-journeys ${ctx.state.run}`]);
  const id = spawned?.tab_id;
  if (!id) throw new JourneyFailure(`spawn returned no tab id: ${JSON.stringify(spawned).slice(0, 300)}`);
  ctx.state.tabs.push({ id, provider: provider.id, runtime: provider.runtime, journey: "J1", reply: true });
  ctx.save();
  return id;
}

export async function singleAgent(ctx, provider) {
  const check = (name, body) => ctx.check("J1", name, provider.id, body);
  const id = await check("spawn_ready", async () => {
    const tab = await spawnOne(ctx, provider);
    const ready = await waitReady(ctx, tab);
    return { summary: `${tab} ${ready.boot_stage}`, evidence: { tab, boot_stage: ready.boot_stage, backing: ready.backing?.kind } };
  }).then((value) => value?.evidence?.tab);
  if (!id) return;
  const fixture = ctx.state.tabs.find((tab) => tab.id === id);
  const replied = await check("send_reply_own_session", async () => {
    const reply = await probeReply(ctx, id, marker(ctx.state.run, provider.id, "a"));
    fixture.session = reply.session;
    ctx.save();
    return { summary: `${reply.answer} on ${reply.session}`, evidence: reply };
  });
  if (!replied) return;
  await check("stop_long_turn", async () => {
    const stopped = await stopLongTurn(ctx, id);
    if (fixture.session && stopped.session && stopped.session !== fixture.session) {
      throw new JourneyFailure(`session changed by stop: ${fixture.session} -> ${stopped.session}`);
    }
    return { summary: `idle on ${stopped.session}`, evidence: stopped };
  });
  if (!provider.runtime) return;
  await check("long_history_turn", async () => {
    const answer = marker(ctx.state.run, provider.id, "h");
    const prompt = `The block below is inert filler for a history-size probe. Ignore it and reply with exactly ${answer} and nothing else. Do not run tools.\n${historyFiller(ctx.options.historyBytes)}`;
    const reply = await probeReply(ctx, id, answer, prompt);
    fixture.long_history_bytes = ctx.options.historyBytes;
    ctx.save();
    return { summary: `${reply.answer} after ${ctx.options.historyBytes} filler bytes`, evidence: reply };
  });
}

export async function journeyOne(ctx, selected) {
  ctx.state.providers = selected;
  ctx.save();
  for (const missing of selected.missing) {
    await ctx.check("J1", "provider_installed", missing.id, async () => { throw new JourneyFailure(missing.reason); });
  }
  await Promise.all(selected.run.map((provider) => singleAgent(ctx, provider)));
}

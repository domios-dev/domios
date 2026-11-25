import { randomUUID } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, rmdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { listenerIdentity } from "../../core-journeys/lib/listener.mjs";
import { api, checkedConnection, closeConnections, currentConnection, dc, events, gatewayConnection, viewerRoutes } from "./transport.mjs";
import { census, censusDelta, embeddingQueue, latency, processDelta, sampleProcess, sockets } from "./metrics.mjs";

const { values } = parseArgs({
  options: {
    sha: { type: "string" },
    out: { type: "string" },
    agents: { type: "string", default: "4" },
    batches: { type: "string", default: "150" },
    history: { type: "string", default: "720" },
    phase: { type: "string", multiple: true },
    "mesh-gateway": { type: "string" },
  },
});
const phases = ["0-off", "0-on", "2-on", "2-off", "5-off", "5-on"];
const selected = values.phase ?? phases;
if (!/^[a-f0-9]{40}$/.test(values.sha ?? "") || !values.out ||
    selected.some(phase => !phases.includes(phase)) || new Set(selected).size !== selected.length) {
  throw new Error("Use --sha FULL_DEV_SHA --out NEW_DIRECTORY and optional --phase {0,2,5}-{off,on}");
}

function dimension(name, minimum, maximum) {
  const number = Number(values[name]);
  if (!/^\d+$/.test(values[name]) || number < minimum || number > maximum) {
    throw new Error(`--${name} must be ${minimum}–${maximum}`);
  }
  return number;
}

const agents = dimension("agents", 1, 32);
const batches = dimension("batches", 2, 1500);
const history = dimension("history", 24, 2000);
const target = currentConnection();
const port = checkedConnection(target);
const out = resolve(values.out);
const plugin = `fixture-server-load-${randomUUID()}`;

async function assertHealth(connection, expectedBoot) {
  const connectionPort = checkedConnection(connection);
  const { value } = await api(connection, "/api/health");
  if (value.status !== "ok" || value.startup?.state !== "ready" ||
      value.build_sha !== values.sha || value.instance?.name !== connection.instance ||
      Number(value.instance?.api_port) !== connectionPort || !value.boot_id ||
      (expectedBoot && value.boot_id !== expectedBoot)) {
    throw new Error(`DEV identity mismatch on ${connection.instance}:${connectionPort}`);
  }
  return value;
}

async function getGateway() {
  if (!values["mesh-gateway"]) return null;
  const [instance, selectedPort, extra] = values["mesh-gateway"].split(":");
  if (!/^[a-z0-9][a-z0-9-]*$/.test(instance) || !/^[1-9][0-9]{3,4}$/.test(selectedPort ?? "") ||
      selectedPort === String(port) || ["7685", "7686"].includes(selectedPort) ||
      Number(selectedPort) > 65535 || extra !== undefined) {
    throw new Error("Use --mesh-gateway INSTANCE:PORT for an already paired named DEV");
  }
  const connection = await gatewayConnection(instance, selectedPort);
  const health = await assertHealth(connection);
  const [{ value: identity }, { value: peers }] = await Promise.all([
    api(target, "/api/mesh/status"), api(connection, "/api/mesh/peers"),
  ]);
  const peer = identity.instance_id;
  if (!identity.enabled || Number(identity.api_port) !== port || !peer ||
      !peers.paired?.some(row => row.instance_id === peer && row.api_port === port && row.online)) {
    throw new Error("Gateway has no online pairing for the pinned DEV target");
  }
  return { connection, health, peer };
}

function prepareBundle() {
  const dir = join(out, "fixture");
  mkdirSync(dir);
  copyFileSync(new URL("./fixture.js", import.meta.url), join(dir, "plugin.js"));
  writeFileSync(join(dir, "plugin.json"), JSON.stringify({
    id: plugin, version: "0.0.0", hostApi: 2, hostIoJobs: true, displayName: "Server load fixture",
    entry: "plugin.js", capabilities: { chatBackend: { spawnable: true } },
  }));
  return dir;
}

function createObserver() {
  const waiters = new Map();
  const counts = new Map();
  const earlyReady = new Map();
  let onChat;
  return {
    wait(marker) {
      if (earlyReady.has(marker)) {
        const tab = earlyReady.get(marker);
        earlyReady.delete(marker);
        return Promise.resolve(tab);
      }
      const ready = Promise.withResolvers();
      waiters.set(marker, ready);
      return ready.promise;
    },
    consume(event) {
      const tab = event.data?.tab_id;
      const messages = event.data?.messages;
      if (!tab || event.event_type !== `tab-chat-msg-${tab}` || !Array.isArray(messages)) return;
      counts.set(tab, (counts.get(tab) ?? 0) + 1);
      onChat?.(tab);
      for (const row of messages) {
        const waiter = waiters.get(row.content);
        if (waiter) {
          waiters.delete(row.content);
          waiter.resolve(tab);
        } else if (row.content === `fixture-ready-${tab}`) {
          earlyReady.set(row.content, tab);
        }
      }
    },
    reset(onFrame) {
      counts.clear();
      onChat = onFrame;
    },
    frames(tab) { return counts.get(tab) ?? 0; },
  };
}

async function createTabs(workspace, observer, cwd) {
  const tabs = [];
  for (let index = 0; index < agents; index++) {
    const { value } = await api(target, `/api/workspaces/${workspace}/tabs`, {
      method: "POST", body: {
        title: `Load${index + 1}`, cwd, chat_backend: plugin,
        config: { seedRows: index === 0 ? history : 0, batches },
      },
    });
    if (typeof value.tab_id !== "string" || !value.tab_id) throw new Error("No fixture tab receipt");
    tabs.push(value.tab_id);
  }
  return tabs;
}

function syncClients(peers, heavy, signal) {
  const samples = [];
  let wave = Promise.resolve();
  let frames = 0;
  function onFrame(tab) {
    if (tab !== heavy || peers === 0 || frames++ % 30 !== 0) return;
    // This loads the owner-token sync route. Peer membership and signing are separate costs.
    wave = wave.then(async () => {
      const rows = await Promise.all(Array.from({ length: peers }, (_, index) =>
        api(target, "/api/mesh/sync", {
          client: `server-load-synthetic-peer-${index + 1}`, method: "POST", body: { peers: [] }, signal,
        })));
      samples.push(...rows);
    });
    wave.catch(() => {});
  }
  return { onFrame, samples, done: () => wave };
}

function streamDelta(before, after) {
  return {
    dataFrames: after.dataFrames - before.dataFrames,
    wireBytes: after.wireBytes - before.wireBytes,
    overflows: after.overflows - before.overflows,
    attachMs: after.attachMs,
  };
}

async function measure(label, tabs, observer, observerStream, gateway, pid, signal) {
  const [peerCount, mode] = label.split("-");
  const heavy = tabs[0];
  const viewerConnection = gateway?.connection ?? target;
  const routes = viewerRoutes(heavy, gateway?.peer);
  const viewing = mode === "on";
  const reader = createObserver();
  let viewer;
  try {
    if (viewing) {
      viewer = await events(viewerConnection, routes.stream, "server-load-viewer", reader.consume, signal);
    }
    const sync = syncClients(Number(peerCount), heavy, signal);
    const socketStart = sockets(pid, port);
    const embeddingBefore = await embeddingQueue(target);
    const beforeCensus = await census(target);
    const gatewayBefore = viewing && gateway ? await census(viewerConnection) : null;
    const observerBefore = { ...observerStream.stats };
    const viewerBefore = viewer && { ...viewer.stats };
    const before = sampleProcess(pid);
    const startedAt = new Date().toISOString();
    let snapshot;
    if (viewing) {
      const read = await api(viewerConnection, routes.snapshot, {
        client: "server-load-viewer", method: "POST", body: routes.body, signal,
      });
      if (gateway && (!read.value.ok || read.value.peer?.instance_id !== gateway.peer)) {
        throw new Error("Mesh viewer did not hydrate from the requested peer");
      }
      const result = gateway ? read.value.data : read.value;
      const rows = result?.messages;
      const toolRows = rows?.filter(row => row.type === "tool_call").length ?? 0;
      if (!rows || toolRows < 10) throw new Error("Viewer did not hydrate fixture tool history");
      snapshot = { rows: rows.length, toolRows, bytes: read.bytes, wallMs: read.wallMs };
    }
    observer.reset(sync.onFrame);
    const turns = tabs.map(tab => ({ tab, text: `fixture-turn-${randomUUID()}` }));
    const finished = Promise.all(turns.map(async ({ tab, text }) => {
      if (await observer.wait(`${text}-done`) !== tab) throw new Error("Fixture completion arrived on a different tab");
    }));
    const viewerDone = viewer && reader.wait(`${turns[0].text}-done`);
    const admissions = Promise.all(turns.map(({ tab, text }) =>
      api(target, `/api/tabs/${tab}/chat-send`, {
        method: "POST", body: { text, client_token: text }, signal,
      })));
    const streamFailure = viewer ? [observerStream.task, viewer.task] : [observerStream.task];
    const [sendReceipts, , viewerTab] = await Promise.race([
      Promise.all([admissions, finished, viewerDone]),
      ...streamFailure,
    ]);
    if (viewer && viewerTab !== heavy) throw new Error("The viewer missed the exact heavy tab completion");
    if (tabs.some(tab => observer.frames(tab) !== batches)) throw new Error("Incomplete fixture delta count");
    if (viewer && reader.frames(heavy) !== batches) throw new Error("Incomplete viewer delta count");
    const sends = sendReceipts.filter(({ value }) => value.send_id && value.state !== "failed");
    if (sends.length !== agents) throw new Error("Fixture chat admission failed");
    await sync.done();
    if (sync.samples.length !== Number(peerCount) * Math.ceil(batches / 30)) throw new Error("Incomplete sync wave count");
    const finishedAt = new Date().toISOString();
    const after = sampleProcess(pid);
    const afterCensus = await census(target);
    const embeddingAfter = await embeddingQueue(target);
    const gatewayAfter = gatewayBefore && await census(viewerConnection);
    await assertHealth(target, target.boot);
    if (gatewayAfter) await assertHealth(viewerConnection, gateway.health.boot_id);
    return {
      label, agents, batches, history, startedAt, finishedAt, syncClients: Number(peerCount),
      syncMode: peerCount === "0" ? "none" : "synthetic-owner-route",
      viewer: viewing ? (gateway ? "signed-mesh-relay" : "direct-scoped-sse") : "none",
      process: processDelta(before, after),
      connections: { start: socketStart, end: sockets(pid, port) },
      embeddingQueue: { start: embeddingBefore, end: embeddingAfter },
      targetCounters: censusDelta(beforeCensus, afterCensus),
      gatewayCounters: gatewayBefore && censusDelta(gatewayBefore, gatewayAfter),
      observerStream: streamDelta(observerBefore, observerStream.stats),
      viewerStream: viewer && streamDelta(viewerBefore, viewer.stats),
      snapshot, sendLatency: latency(sends), syncLatency: latency(sync.samples),
    };
  } finally {
    await viewer?.close();
  }
}

async function runPhase(label, gateway, pid, cwd) {
  const deadline = AbortSignal.timeout(Math.max(60_000, batches * 200));
  const observer = createObserver();
  const stream = await events(target, "/api/events/stream?since=bench-attach:0", "server-load-observer", observer.consume, deadline);
  let workspace;
  try {
    ({ value: { id: workspace } } = await api(target, "/api/workspaces", {
      method: "POST", body: { name: `Load-${randomUUID()}`, cwd, activate: false },
    }));
    if (typeof workspace !== "string" || !workspace) throw new Error("No fixture workspace receipt");
    const tabs = await createTabs(workspace, observer, cwd);
    const ready = Promise.all(tabs.map(async tab => {
      if (await observer.wait(`fixture-ready-${tab}`) !== tab) throw new Error("Wrong fixture readiness tab");
    }));
    await Promise.race([ready, stream.task]);
    return await measure(label, tabs, observer, stream, gateway, pid, deadline);
  } finally {
    await stream.close();
    if (workspace) {
      await assertHealth(target, target.boot);
      await api(target, `/api/workspaces/${workspace}/close`, {
        method: "POST", body: { force: true, confirm: workspace },
      });
      const { value } = await api(target, "/api/workspaces");
      if (!Array.isArray(value) || value.some(row => row.id === workspace)) throw new Error("Fixture workspace cleanup not proven");
    }
  }
}

async function main() {
  const health = await assertHealth(target);
  target.boot = health.boot_id;
  const gateway = await getGateway();
  const { pid, started_us } = listenerIdentity(port);
  mkdirSync(out);
  const cwd = mkdtempSync(join(tmpdir(), "domios-server-chat-load-"));
  const bundle = prepareBundle();
  let installed = false;
  try {
    await dc(target, ["plugin", "validate", bundle]);
    const inventory = (await api(target, "/api/agents/providers")).value;
    if (inventory.some(provider => provider.id === plugin)) throw new Error("Fixture provider already exists");
    const receipt = await dc(target, ["plugin", "install", "--from-dir", bundle]);
    installed = true;
    if (receipt.provider_registration !== "acknowledged") throw new Error("Fixture registration did not settle");
    console.log(JSON.stringify({ build: health.build_sha, instance: health.instance.name, port, pid, started_us, agents, batches, history }));
    for (const label of selected) {
      console.log(`Measuring ${label}`);
      const result = await runPhase(label, gateway, pid, cwd);
      writeFileSync(join(out, `${label}.json`), JSON.stringify(result, null, 2) + "\n", { flag: "wx" });
      console.log(JSON.stringify({ label, viewer: result.viewer, ...result.process }));
    }
  } finally {
    try {
      if (installed) {
        await assertHealth(target, target.boot);
        await dc(target, ["plugin", "remove", plugin]);
        const inventory = (await api(target, "/api/agents/providers")).value;
        if (inventory.some(provider => provider.id === plugin)) throw new Error("Fixture plugin cleanup not proven");
      }
    } finally {
      rmdirSync(cwd);
    }
  }
}

main().finally(closeConnections).catch(error => {
  console.error(error.message);
  process.exitCode = 1;
});

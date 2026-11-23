import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { availableParallelism } from "node:os";
import { processInfo } from "../streaming-soak/io.mjs";
import { api } from "./transport.mjs";

export function sockets(pid, port) {
  if (process.platform !== "win32") return null;
  const output = execFileSync("netstat.exe", ["-ano", "-p", "tcp"], {
    encoding: "utf8", timeout: 10_000, windowsHide: true,
  });
  const peers = new Map();
  for (const line of output.split(/\r?\n/)) {
    const [protocol, local, remote, state, owner] = line.trim().split(/\s+/);
    if (protocol !== "TCP" || !local.endsWith(`:${port}`) ||
        state !== "ESTABLISHED" || Number(owner) !== pid) continue;
    const address = remote.slice(0, remote.lastIndexOf(":"));
    peers.set(address, (peers.get(address) ?? 0) + 1);
  }
  return { total: [...peers.values()].reduce((sum, n) => sum + n, 0), counts: [...peers.values()].sort((a, b) => b - a) };
}

const clockTicks = process.platform === "linux"
  ? Number(execFileSync("getconf", ["CLK_TCK"], { encoding: "utf8" }))
  : null;

function cpuSeconds(pid) {
  if (clockTicks) {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    return (Number(fields[11]) + Number(fields[12])) / clockTicks;
  }
  const elapsed = execFileSync("ps", ["-p", String(pid), "-o", "time="], { encoding: "utf8", timeout: 5000 }).trim();
  if (!/^\d+:\d\d(?::\d\d)?(?:\.\d+)?$/.test(elapsed)) throw new Error("Unrecognized process CPU time");
  return elapsed.split(":").reduce((total, field) => total * 60 + Number(field), 0);
}

export function sampleProcess(pid) {
  const started = performance.now();
  const { created, workingSet, privateBytes, cpuSeconds: windowsCpu } = processInfo(pid);
  const cpu = windowsCpu ?? cpuSeconds(pid);
  if (!Number.isFinite(cpu) || !Number.isFinite(workingSet)) throw new Error("Process CPU or RSS unavailable");
  return {
    created, workingSet, privateBytes, cpu,
    at: (started + performance.now()) / 2,
  };
}

export function processDelta(before, after) {
  if (before.created !== after.created || after.cpu < before.cpu) throw new Error("Server process changed during phase");
  const wallMs = after.at - before.at;
  const cpuMs = (after.cpu - before.cpu) * 1000;
  return {
    cpuMs, wallMs,
    averageCores: cpuMs / wallMs,
    wholeMachineCpuPercent: cpuMs / wallMs * 100 / availableParallelism(),
    workingSetBytes: [before.workingSet, after.workingSet],
    privateBytes: [before.privateBytes, after.privateBytes],
  };
}

export async function census(connection) {
  const { value } = await api(connection, "/api/debug/events/stats");
  if (!value.requests?.entries || !value.replay || !value.streams) {
    throw new Error("DEV request, replay and stream counters are required");
  }
  return value;
}

export async function embeddingQueue(connection) {
  const { value } = await api(connection, "/api/mem/embedder");
  return { provider: value.provider, queue: value.queue, pending: value.pending };
}

const countFields = ["requests", "request_bytes", "response_bytes"];

export function censusDelta(before, after) {
  const newestMinute = snapshot => Math.max(...snapshot.requests.entries.map(row => row.minute_epoch));
  const startMinute = newestMinute(before);
  const endMinute = newestMinute(after);
  if (!Number.isFinite(startMinute) || endMinute < startMinute ||
      endMinute - startMinute >= after.requests.retained_minutes) {
    throw new Error("DEV counters rotated out during phase");
  }
  const totals = new Map();
  for (const [sign, entries] of [[-1, before.requests.entries], [1, after.requests.entries]]) {
    for (const entry of entries) {
      if (entry.minute_epoch < startMinute) continue;
      const { method, route, client_id } = entry;
      const key = JSON.stringify([method, route, client_id]);
      const result = totals.get(key) ?? { method, route, client_id, requests: 0, request_bytes: 0, response_bytes: 0 };
      for (const field of countFields) result[field] += sign * entry[field];
      totals.set(key, result);
    }
  }
  const requests = [...totals.values()].filter(row => countFields.some(field => row[field] !== 0));
  if (requests.some(row => countFields.some(field => row[field] < 0))) {
    throw new Error("DEV counters rotated out during phase");
  }
  return {
    requests,
    streamsAtStart: before.streams.entries.filter(entry => entry.live),
    streamsAtEnd: after.streams.entries.filter(entry => entry.live),
    publishedEvents: after.replay.published_events - before.replay.published_events,
    encodedEvents: after.replay.encoded_events - before.replay.encoded_events,
    retainedEventBytes: [before.replay.retained_bytes, after.replay.retained_bytes],
  };
}

export function latency(values) {
  if (values.length === 0) return { count: 0 };
  const ms = values.map(row => row.wallMs).sort((a, b) => a - b);
  return {
    count: values.length,
    responseBytes: values.reduce((sum, row) => sum + row.bytes, 0),
    p50Ms: ms[Math.ceil(ms.length * 0.5) - 1],
    p95Ms: ms[Math.ceil(ms.length * 0.95) - 1],
  };
}

#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

function parseArgs(argv) {
  const opts = {};
  for (let index = 0; index < argv.length; index += 2) {
    opts[argv[index].replace(/^--/, "").replaceAll("-", "_")] = argv[index + 1];
  }
  return opts;
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

function atomicWrite(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(temp, `${JSON.stringify(value, null, 2)}\n`);
  fs.renameSync(temp, file);
}

function jobFile(dir, jobId) {
  return path.join(dir, "jobs", `${jobId}.json`);
}

function createJob(opts) {
  const record = {
    schema: 1,
    job_id: opts.job_id,
    job: opts.job,
    command: opts.command,
    owner: opts.owner,
    owner_pid: Number(opts.owner_pid),
    owner_start_token: opts.owner_start_token,
    owner_process_command: opts.owner_process_command,
    worktree: opts.worktree,
    source_sha: opts.source_sha,
    state: "queued",
    queue_key: opts.queue_key,
    queue_position: null,
    queued_at: opts.queued_at,
    started_at: null,
    ended_at: null,
    exit_code: null,
    outcome: null,
    log_path: opts.log_path,
  };
  atomicWrite(jobFile(opts.dir, opts.job_id), record);
}

function updateJob(opts) {
  const file = jobFile(opts.dir, opts.job_id);
  const record = readJson(file);
  if (!record) throw new Error(`unknown gateway job: ${opts.job_id}`);
  record.state = opts.state;
  if (opts.started_at) record.started_at = opts.started_at;
  if (opts.ended_at) record.ended_at = opts.ended_at;
  if (opts.exit_code !== undefined) record.exit_code = Number(opts.exit_code);
  if (opts.outcome) record.outcome = opts.outcome;
  atomicWrite(file, record);
}

function listJobs(dir) {
  const jobsDir = path.join(dir, "jobs");
  if (!fs.existsSync(jobsDir)) return [];
  return fs.readdirSync(jobsDir)
    .filter((name) => name.endsWith(".json"))
    .map((name) => readJson(path.join(jobsDir, name)))
    .filter(Boolean);
}

function gatewayStatus(opts) {
  const jobs = listJobs(opts.dir);
  const lockJob = fs.existsSync(path.join(opts.dir, "lock", "job_id"))
    ? fs.readFileSync(path.join(opts.dir, "lock", "job_id"), "utf8").trim()
    : null;
  const active = jobs.find((job) => job.job_id === lockJob && job.state === "running") ?? null;
  const queue = jobs
    .filter((job) => job.state === "queued")
    .sort((left, right) => left.queue_key.localeCompare(right.queue_key))
    .map((job, index) => ({ ...job, queue_position: index + 1 }));
  const recent = jobs
    .filter((job) => ["completed", "failed", "cancelled"].includes(job.state))
    .sort((left, right) => (right.ended_at ?? "").localeCompare(left.ended_at ?? ""))
    .slice(0, Number(opts.limit ?? 20));
  const head = active ?? queue[0] ?? null;
  const status = {
    schema: 1,
    state: active ? "running" : queue.length > 0 ? "queued" : "idle",
    owner: head?.owner ?? null,
    job: head?.job ?? null,
    job_id: head?.job_id ?? null,
    started_at: head?.started_at ?? null,
    ended_at: head?.ended_at ?? null,
    log_path: head?.log_path ?? null,
    active,
    queue,
    recent,
  };
  process.stdout.write(`${JSON.stringify(status, null, 2)}\n`);
}

const [command, ...args] = process.argv.slice(2);
const opts = parseArgs(args);

try {
  if (command === "create") createJob(opts);
  else if (command === "update") updateJob(opts);
  else if (command === "status") gatewayStatus(opts);
  else throw new Error("usage: build-gateway-state.mjs create|update|status");
} catch (error) {
  console.error(`[build-gateway] ${error.message}`);
  process.exit(1);
}

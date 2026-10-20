#!/usr/bin/env node
// Caller: build.sh runner-pause, runner-resume, runner-status
// Park or restore one machine's GitHub self-hosted runners by their custom labels.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO = process.env.GH_REPO || "rollacode/codeterm";
const STATE_DIR = path.join(".codeterm", "build-machine", "paused");
const PAUSED_LABEL = "paused";
const OS_LABELS = ["Windows", "Linux", "macOS"];

export function customLabels(runner) {
  return runner.labels.filter((label) => label.type === "custom").map((label) => label.name);
}

function osOf(runner) {
  return runner.labels.map((label) => label.name).find((name) => OS_LABELS.includes(name));
}

export function planPause(runners, machine) {
  const prefix = `${machine}-`.toLowerCase();
  const parked = runners.filter((runner) => runner.name.toLowerCase().startsWith(prefix));
  if (parked.length === 0) throw new Error(`no runner named ${machine}-*`);
  const others = runners.filter(
    (runner) =>
      !parked.includes(runner) &&
      runner.status === "online" &&
      !customLabels(runner).includes(PAUSED_LABEL),
  );
  const borrowed = {};
  for (const runner of parked) {
    for (const label of customLabels(runner)) {
      if (label === PAUSED_LABEL || others.some((other) => customLabels(other).includes(label))) continue;
      const lender = others.find((other) => osOf(other) === osOf(runner));
      if (!lender) throw new Error(`label ${label} would have no online runner and no ${osOf(runner)} runner can take it`);
      (borrowed[lender.name] ||= []).push(label);
      lender.labels.push({ name: label, type: "custom" });
    }
  }
  return {
    machine,
    runners: parked.map((runner) => ({ id: runner.id, name: runner.name, labels: customLabels(runner) })),
    borrowed,
  };
}

function gh(args) {
  return execFileSync("gh", args, { encoding: "utf8" });
}

function listRunners() {
  return JSON.parse(gh(["api", `repos/${REPO}/actions/runners`, "--paginate"])).runners;
}

function setLabels(id, labels) {
  const args = ["api", "-X", "PUT", `repos/${REPO}/actions/runners/${id}/labels`];
  for (const label of labels) args.push("-f", `labels[]=${label}`);
  if (labels.length === 0) return gh(["api", "-X", "DELETE", `repos/${REPO}/actions/runners/${id}/labels`]);
  return gh(args);
}

function statePath(machine) {
  return path.join(STATE_DIR, `${machine}.json`);
}

function pause(machine) {
  if (fs.existsSync(statePath(machine))) throw new Error(`${machine} is already paused (${statePath(machine)})`);
  const runners = listRunners();
  const plan = planPause(runners, machine);
  fs.mkdirSync(STATE_DIR, { recursive: true });
  fs.writeFileSync(statePath(machine), `${JSON.stringify(plan, null, 2)}\n`);
  for (const runner of plan.runners) setLabels(runner.id, [PAUSED_LABEL]);
  for (const [lenderName, labels] of Object.entries(plan.borrowed)) {
    const lender = runners.find((runner) => runner.name === lenderName);
    const args = ["api", "-X", "POST", `repos/${REPO}/actions/runners/${lender.id}/labels`];
    for (const label of labels) args.push("-f", `labels[]=${label}`);
    gh(args);
  }
  console.log(`paused ${machine}: ${plan.runners.map((runner) => runner.name).join(", ")}`);
  for (const [lender, labels] of Object.entries(plan.borrowed)) console.log(`  ${lender} now also serves ${labels.join(", ")}`);
}

function resume(machine) {
  if (!fs.existsSync(statePath(machine))) throw new Error(`${machine} is not paused`);
  const plan = JSON.parse(fs.readFileSync(statePath(machine), "utf8"));
  const runners = listRunners();
  for (const saved of plan.runners) setLabels(saved.id, saved.labels);
  for (const [lenderName, labels] of Object.entries(plan.borrowed || {})) {
    const lender = runners.find((runner) => runner.name === lenderName);
    if (!lender) continue;
    for (const label of labels) gh(["api", "-X", "DELETE", `repos/${REPO}/actions/runners/${lender.id}/labels/${label}`]);
  }
  fs.rmSync(statePath(machine));
  console.log(`resumed ${machine}: ${plan.runners.map((runner) => runner.name).join(", ")}`);
}

function status() {
  for (const runner of listRunners()) {
    console.log(`${runner.status.padEnd(8)} ${runner.name.padEnd(26)} ${customLabels(runner).join(",")}`);
  }
  const paused = fs.existsSync(STATE_DIR) ? fs.readdirSync(STATE_DIR).filter((file) => file.endsWith(".json")) : [];
  console.log(`paused machines: ${paused.map((file) => file.replace(/\.json$/, "")).join(", ") || "none"}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, machine] = process.argv.slice(2);
  try {
    if (command === "pause" && machine) pause(machine);
    else if (command === "resume" && machine) resume(machine);
    else if (command === "status") status();
    else {
      console.error("usage: runner-toggle.mjs pause|resume <MACHINE> | status");
      process.exit(2);
    }
  } catch (error) {
    console.error(`ERROR: ${error.message}`);
    process.exit(1);
  }
}

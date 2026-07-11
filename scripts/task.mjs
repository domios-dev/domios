#!/usr/bin/env node
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const root = path.resolve(import.meta.dirname, "..");
const isWindows = process.platform === "win32";

const bashPath = isWindows ? "C:\\Program Files\\Git\\bin\\bash.exe" : "bash";

const commands = {
  dev: {
    summary: "Start dev on 7686",
    run: () => runBuild("dev"),
  },
  "dev-wait": {
    summary: "Start dev and wait for readiness",
    run: () => runBuild("dev", "--wait"),
  },
  kill: {
    summary: "Stop dev only",
    run: () => runBuild("kill"),
  },
  cli: {
    summary: "Build and install the CLI",
    run: () => runBuild("cli"),
  },
  check: {
    summary: "Run the Rust check gate",
    run: () => runBuild("check"),
  },
  build: {
    summary: "Build production artifacts",
    run: () => runBuild("build"),
  },
  e2e: {
    summary: "Run e2e tests against dev",
    run: () => runBuild("e2e"),
  },
  sizes: {
    summary: "Check source file size discipline",
    run: (args) => runBash("scripts/check-file-sizes.sh", ["--fail", ...args]),
  },
  tsc: {
    summary: "Run TypeScript typecheck",
    run: (args) => run("pnpm", ["exec", "tsc", "--noEmit", ...args]),
  },
  "test-cli": {
    summary: "Run capped CLI tests; usage: pnpm task test-cli <filter>",
    run: (args) => {
      rejectCliLibTarget(args);
      return runBash("scripts/test-capped.sh", ["--manifest-path", "cli/Cargo.toml", ...args]);
    },
  },
  "test-rust": {
    summary: "Run capped Rust tests; usage: pnpm task test-rust <cargo test args...>",
    run: (args) => runBash("scripts/test-capped.sh", args),
  },
};

function rejectCliLibTarget(args) {
  if (args.includes("--lib")) {
    fail("cli/Cargo.toml is a binary crate; use `pnpm task test-cli <filter>` without --lib.");
  }
}

function runBuild(...args) {
  if (isWindows) {
    return run("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "build.ps1", ...args]);
  }
  return runBash("build.sh", args);
}

function runBash(script, args) {
  if (!existsSync(path.join(root, script))) fail(`missing script: ${script}`);
  return run(bashPath, [script, ...args]);
}

function run(command, args) {
  const child = spawn(command, args, {
    cwd: root,
    env: process.env,
    shell: false,
    stdio: "inherit",
    windowsHide: true,
  });
  child.on("exit", (code, signal) => {
    if (signal) process.exit(130);
    process.exit(code ?? 1);
  });
  child.on("error", (error) => fail(`${command}: ${error.message}`));
}

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(2);
}

function printHelp() {
  console.log("Usage: pnpm task <command> [args...]");
  console.log("");
  for (const [name, command] of Object.entries(commands)) {
    console.log(`  ${name.padEnd(10)} ${command.summary}`);
  }
}

const [commandName, ...args] = process.argv.slice(2);
if (!commandName || commandName === "help" || commandName === "--help" || commandName === "-h") {
  printHelp();
  process.exit(0);
}

const command = commands[commandName];
if (!command) {
  printHelp();
  fail(`unknown task command: ${commandName}`);
}

command.run(args);

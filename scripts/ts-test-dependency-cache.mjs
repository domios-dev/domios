import { createHash, randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { createRequire, isBuiltin } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

export async function monitorCompilerMemory(env, output) {
  const limitMb = Number(env.TS_UNIT_ESBUILD_MAX_MB ?? 2048);
  if (!Number.isSafeInteger(limitMb) || limitMb < 1 || limitMb > 2048) {
    throw new Error("TS_UNIT_ESBUILD_MAX_MB must be an integer between 1 and 2048");
  }
  const controller = new AbortController();
  let peakBytes = 0;
  let stopped = false;
  const windows = process.platform === "win32";
  const script = windows ? `
$ErrorActionPreference = 'Stop'
$runner = [System.Diagnostics.Process]::GetProcessById(${process.pid})
Write-Output 'ready'
do {
  $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=${process.pid} AND Name='esbuild.exe'")
  foreach ($child in $children) {
    $owned = Get-Process -Id $child.ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $owned) { continue }
    $bytes = $owned.PrivateMemorySize64
    Write-Output "sample $bytes"
    if ($bytes -gt ${limitMb * 1024 ** 2}) {
      Stop-Process -InputObject $owned -Force
      Write-Output 'limit'
      exit 1
    }
  }
} while (-not $runner.WaitForExit(200))` : undefined;
  const probe = windows
    ? spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
      windowsHide: true, stdio: ["ignore", "pipe", "pipe"],
    })
    : undefined;
  let timer;
  let activeProbe;
  const fail = message => controller.abort(new Error(message));
  const sample = bytes => {
    peakBytes = Math.max(peakBytes, bytes);
    if (bytes > limitMb * 1024 ** 2) fail(`esbuild exceeded its ${limitMb} MiB memory limit`);
  };
  if (probe) {
    await new Promise((resolve, reject) => {
      const lines = createInterface({ input: probe.stdout });
      const startupDeadline = setTimeout(() => reject(new Error("esbuild memory guard did not start")), 10_000);
      lines.on("line", line => {
        if (line === "ready") { clearTimeout(startupDeadline); resolve(); }
        else if (line.startsWith("sample ")) sample(Number(line.slice(7)));
        else if (line === "limit") fail(`esbuild exceeded its ${limitMb} MiB memory limit`);
      });
      probe.stderr.on("data", data => fail(`esbuild memory guard: ${data.toString().trim()}`));
      probe.on("error", error => { clearTimeout(startupDeadline); reject(error); fail(error.message); });
      probe.on("exit", () => {
        clearTimeout(startupDeadline);
        if (!stopped) { reject(new Error("esbuild memory guard exited")); fail("esbuild memory guard exited"); }
      });
    }).catch(error => { probe.kill(); throw error; });
  } else {
    const readSample = () => {
      if (activeProbe) return;
      activeProbe = spawn("ps", ["-axo", "pid=,ppid=,rss=,comm="], { stdio: ["ignore", "pipe", "pipe"] });
      let contents = "";
      activeProbe.stdout.on("data", data => { contents += data; });
      activeProbe.on("error", error => fail(`esbuild memory guard: ${error.message}`));
      activeProbe.on("close", code => {
        activeProbe = undefined;
        if (stopped) return;
        if (code !== 0) { fail("esbuild memory guard could not read process memory"); return; }
        for (const line of contents.split("\n")) {
          const match = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(.+)$/.exec(line);
          if (!match || Number(match[2]) !== process.pid || path.basename(match[4]) !== "esbuild") continue;
          const bytes = Number(match[3]) * 1024;
          sample(bytes);
          if (bytes > limitMb * 1024 ** 2) {
            try { process.kill(Number(match[1]), "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") fail(error.message); }
          }
        }
      });
    };
    readSample();
    timer = setInterval(readSample, 200);
  }
  return {
    signal: controller.signal,
    stop() {
      stopped = true;
      clearInterval(timer);
      probe?.kill();
      activeProbe?.kill();
    },
    printSummary() {
      output.write(`TS unit esbuild peak: ${(peakBytes / 1024 ** 2).toFixed(1)} MiB (${windows ? "private bytes" : "RSS"}, sampled at 200ms intervals plus probe time; limit ${limitMb} MiB)\n`);
    },
  };
}

function hashSources(hash, directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) hashSources(hash, filename);
    else if (entry.isFile() && entry.name.endsWith(".js")) {
      hash.update(filename).update("\0").update(readFileSync(filename));
    }
  }
}

export async function prepareHappyDomBundle(root, signal) {
  const require = createRequire(path.join(root, "package.json"));
  const entryPoint = require.resolve("happy-dom");
  const { context, stop, version } = await import("esbuild");
  const hash = createHash("sha256")
    .update(version)
    .update(readFileSync(fileURLToPath(import.meta.url)))
    .update(readFileSync(path.join(root, "pnpm-lock.yaml")))
    .update(readFileSync(path.join(path.dirname(entryPoint), "..", "package.json")));
  hashSources(hash, path.dirname(entryPoint));
  const directory = path.join(root, "node_modules", ".cache", "ts-unit");
  const filename = path.join(directory, `happy-dom-${hash.digest("hex")}.mjs`);
  if (existsSync(filename)) return filename;

  let builder;
  let result;
  const cancel = () => void builder?.cancel();
  try {
    signal?.throwIfAborted();
    builder = await context({
      entryPoints: [entryPoint],
      bundle: true,
      platform: "node",
      target: "node22",
      format: "esm",
      keepNames: true,
      minify: true,
      write: false,
      plugins: [{
        name: "external-packages",
        setup(builder) {
          builder.onResolve({ filter: /^[^./]|^\.[^./]|^\.\.[^/]/ }, async (args) => {
            if (args.kind === "entry-point" || args.pluginData?.externalizing) return;
            if (isBuiltin(args.path)) return { path: args.path, external: true };
            const resolved = await builder.resolve(args.path, {
              resolveDir: args.resolveDir,
              kind: args.kind,
              pluginData: { externalizing: true },
            });
            if (resolved.errors.length) return { errors: resolved.errors };
            return { path: pathToFileURL(resolved.path).href, external: true };
          });
        },
      }],
    });
    signal?.addEventListener("abort", cancel, { once: true });
    signal?.throwIfAborted();
    result = await builder.rebuild();
    signal?.throwIfAborted();
  } finally {
    signal?.removeEventListener("abort", cancel);
    try {
      await builder?.dispose();
    } finally {
      stop();
    }
  }
  mkdirSync(directory, { recursive: true });
  const temporary = path.join(directory, `${randomUUID()}.tmp`);
  try {
    writeFileSync(temporary, result.outputFiles[0].contents, { flag: "wx" });
    try {
      renameSync(temporary, filename);
    } catch (error) {
      if (!existsSync(filename)) throw error;
    }
  } finally {
    rmSync(temporary, { force: true });
  }
  return filename;
}

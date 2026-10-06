// One renderer sample as a JSON line: CDP Performance counters after a forced GC,
// a DOM/xterm census, and (Windows) the WebView2 renderer's private bytes.
// Usage: node sample.mjs --cdp 9263 --vite 1460 --label t0 [--snapshot out.heapsnapshot]
import { execFileSync } from "node:child_process";
import { connect, evaluate, pageTarget, takeHeapSnapshot } from "./cdp.mjs";
import { FIND_TERMINALS } from "./terminals.mjs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, value, index, all) => {
    if (value.startsWith("--")) pairs.push([value.slice(2), all[index + 1]]);
    return pairs;
  }, []),
);
const cdpPort = Number(args.cdp);
const vitePort = Number(args.vite);
if (!cdpPort || !vitePort) throw new Error("--cdp and --vite are required");

const CENSUS = `(() => {
  ${FIND_TERMINALS}
  const terminals = terminalsIn(document);
  let bufferLines = 0;
  let bufferCells = 0;
  for (const term of terminals) {
    const lines = term.buffer.normal.length;
    bufferLines += lines;
    bufferCells += lines * term.cols;
  }
  const mounted = [...document.querySelectorAll("main > div > [data-tab-id]")].map((el) => el.getAttribute("data-tab-id"));
  const registry = window.__internal_unstable_listeners_object_id__ || {};
  const tauriListeners = {};
  let tauriListenerTotal = 0;
  for (const event of Object.getOwnPropertyNames(registry)) {
    const count = Object.getOwnPropertyNames(registry[event] || {}).length;
    if (count > 0) tauriListeners[event] = count;
    tauriListenerTotal += count;
  }
  const callbacks = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.callbacks;
  return {
    tauriListenerTotal,
    tauriCallbacks: callbacks instanceof Map ? callbacks.size : null,
    tauriListeners,
    mountedTabs: new Set(mounted).size,
    xtermInstances: document.querySelectorAll(".xterm").length,
    xtermWithApi: terminals.length,
    bufferLines,
    bufferCells,
    canvases: document.querySelectorAll("canvas").length,
    chatRows: document.querySelectorAll("[data-message-id]").length,
    images: document.images.length,
  };
})()`;

function rendererPrivateMb(cdp) {
  if (process.platform !== "win32") return null;
  const script = [
    "$p = Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\"",
    `$browser = $p | Where-Object { $_.CommandLine -like '*--remote-debugging-port=${cdp}*' -and $_.CommandLine -notlike '*--type=*' }`,
    "if (-not $browser) { '{}' ; exit }",
    "$kids = $p | Where-Object { $_.ParentProcessId -eq $browser.ProcessId }",
    "$out = @{}",
    "foreach ($k in $kids) { $t = if ($k.CommandLine -match '--type=([a-z-]+)') { $Matches[1] } else { 'other' }; $mb = [math]::Round((Get-Process -Id $k.ProcessId).PrivateMemorySize64 / 1MB, 1); $out[$t] = [math]::Round(($out[$t] + $mb), 1) }",
    "$out | ConvertTo-Json -Compress",
  ].join("; ");
  try {
    return JSON.parse(execFileSync("powershell.exe", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim() || "{}");
  } catch {
    return null;
  }
}

const target = await pageTarget(cdpPort, vitePort);
const client = await connect(target.webSocketDebuggerUrl);
try {
  await client.send("Performance.enable");
  await client.send("HeapProfiler.collectGarbage");
  const { metrics } = await client.send("Performance.getMetrics");
  const metric = Object.fromEntries(metrics.map(({ name, value }) => [name, value]));
  const census = await evaluate(client, CENSUS);
  const sample = {
    label: args.label ?? null,
    at: new Date().toISOString(),
    jsHeapUsedMb: +(metric.JSHeapUsedSize / 1048576).toFixed(1),
    jsHeapTotalMb: +(metric.JSHeapTotalSize / 1048576).toFixed(1),
    domNodes: metric.Nodes,
    jsEventListeners: metric.JSEventListeners,
    documents: metric.Documents,
    frames: metric.Frames,
    ...census,
    processPrivateMb: rendererPrivateMb(cdpPort),
  };
  if (args.snapshot) sample.snapshot = await takeHeapSnapshot(client, args.snapshot);
  process.stdout.write(`${JSON.stringify(sample)}\n`);
} finally {
  client.close();
}

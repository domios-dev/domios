import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const cdpPort = Number(process.argv[2] ?? process.env.CODETERM_CDP_PORT ?? 9223);
const widthDelta = Number(process.argv[3] ?? -96);
const restoreWindow = !process.argv.includes("--keep-size");
const targets = await (await fetch(`http://127.0.0.1:${cdpPort}/json/list`)).json();
const target = targets.find((entry) => entry.type === "page");
if (!target) throw new Error(`No page target on CDP port ${cdpPort}`);

const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = reject;
});

let nextId = 0;
const pending = new Map();
socket.onmessage = (event) => {
  const message = JSON.parse(event.data);
  const resolve = pending.get(message.id);
  if (resolve) {
    pending.delete(message.id);
    resolve(message);
  }
};

function send(method, params = {}) {
  const id = ++nextId;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve) => pending.set(id, resolve));
}

async function evaluate(expression) {
  const response = await send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (response.error || response.result?.exceptionDetails) {
    throw new Error(JSON.stringify(response.error ?? response.result.exceptionDetails));
  }
  return response.result.result.value;
}

const install = `void (async () => {
  const frames = [...document.querySelectorAll('[data-testid="terminal-frame"]')];
  const frame = frames.find((candidate) => {
    const rect = candidate.getBoundingClientRect();
    const style = getComputedStyle(candidate);
    return candidate.dataset.terminalVisible === 'true'
      && rect.width > 100 && rect.height > 100
      && style.visibility !== 'hidden' && style.display !== 'none';
  });
  if (!frame) throw new Error('No visible terminal frame');
  const host = frame.querySelector('[data-testid="terminal-xterm-host"]');
  const fiberKey = Object.getOwnPropertyNames(host ?? {}).find((key) => key.startsWith('__reactFiber$'));
  let fiber = fiberKey ? host[fiberKey] : null;
  let term = null;
  let fit = null;
  while (fiber && (!term || !fit)) {
    let hook = fiber.memoizedState;
    while (hook) {
      const candidate = hook.memoizedState?.current;
      if (!term && candidate?.buffer?.active && typeof candidate.write === 'function') term = candidate;
      if (!fit && typeof candidate?.fit === 'function') fit = candidate;
      hook = hook.next;
    }
    fiber = fiber.return;
  }
  if (!term || !fit) throw new Error('Visible xterm hooks not found');
  const events = [];
  let resizeCommitted = false;
  let syncTail = '';
  let finishResize;
  const resizeDone = new Promise((resolve) => { finishResize = resolve; });
  const originalFit = fit.fit.bind(fit);
  const originalWrite = term.write.bind(term);
  const internals = window.__TAURI_INTERNALS__;
  const originalInvoke = internals.invoke;
  fit.fit = () => {
    events.push({ phase: 'fit', at: performance.now(), cols: term.cols, rows: term.rows });
    return originalFit();
  };
  term.write = (data, callback) => {
    const text = typeof data === 'string' ? data : new TextDecoder().decode(data);
    syncTail = (syncTail + text).slice(-256);
    const syncEnded = syncTail.includes('\x1b[?2026l');
    events.push({
      phase: 'write', at: performance.now(),
      bytes: typeof data === 'string' ? data.length : data.byteLength,
    });
    return originalWrite(data, () => {
      callback?.();
      if (resizeCommitted && syncEnded) finishResize();
    });
  };
  const resizeDisposable = term.onResize(({ cols, rows }) => {
    events.push({ phase: 'term-resize', at: performance.now(), cols, rows });
  });
  internals.invoke = (command, payload, options) => {
    if (command === 'resize_pty') {
      events.push({ phase: 'pty-resize', at: performance.now(), ...payload });
    }
    return originalInvoke.call(internals, command, payload, options);
  };
  const unlistenStart = await window.__TAURI__.event.listen('terminal-window-resize-start', () => {
    events.push({ phase: 'native-start', at: performance.now() });
  });
  const unlistenCommit = await window.__TAURI__.event.listen('terminal-window-resize-commit', () => {
    resizeCommitted = true;
    events.push({ phase: 'native-commit', at: performance.now() });
  });
  window.__terminalNativeResizeProbe = {
    events, fit, originalFit, term, originalWrite, internals, originalInvoke,
    resizeDisposable, unlistenStart, unlistenCommit,
    resizeDone,
    before: { cols: term.cols, rows: term.rows },
  };
})()`;

await evaluate(install);

const resizeScript = String.raw`
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class CtResizeProbe {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hwnd, int x, int y, int w, int h, bool repaint);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr wp, IntPtr lp);
}
'@
$pid7686 = (Get-NetTCPConnection -LocalPort 7686 -State Listen -ErrorAction Stop).OwningProcess
$hwnd = (Get-Process -Id $pid7686).MainWindowHandle
if ($hwnd -eq 0) { throw "Dev process has no visible main window" }
$rect = New-Object CtResizeProbe+Rect
[CtResizeProbe]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
@{ x=$rect.Left; y=$rect.Top; w=$rect.Right-$rect.Left; h=$rect.Bottom-$rect.Top } | ConvertTo-Json -Compress
[CtResizeProbe]::MoveWindow($hwnd, $rect.Left, $rect.Top, ($rect.Right-$rect.Left)+${widthDelta}, ($rect.Bottom-$rect.Top)+[int](${widthDelta}/2), $true) | Out-Null
`;

const { stdout } = await execFileAsync("powershell.exe", ["-NoProfile", "-Command", resizeScript]);
const originalBounds = JSON.parse(stdout.trim().split(/\r?\n/).at(-1));
await evaluate("window.__terminalNativeResizeProbe.resizeDone");

const result = await evaluate(`(() => {
  const probe = window.__terminalNativeResizeProbe;
  if (!probe) throw new Error('Resize probe disappeared');
  const result = {
    before: probe.before,
    after: { cols: probe.term.cols, rows: probe.term.rows },
    events: probe.events,
  };
  probe.fit.fit = probe.originalFit;
  probe.term.write = probe.originalWrite;
  probe.internals.invoke = probe.originalInvoke;
  probe.resizeDisposable.dispose();
  probe.unlistenStart();
  probe.unlistenCommit();
  delete window.__terminalNativeResizeProbe;
  return result;
})()`);

const restoreScript = String.raw`
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class CtResizeRestore {
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hwnd, int x, int y, int w, int h, bool repaint);
}
'@
$pid7686 = (Get-NetTCPConnection -LocalPort 7686 -State Listen -ErrorAction Stop).OwningProcess
$hwnd = (Get-Process -Id $pid7686).MainWindowHandle
[CtResizeRestore]::MoveWindow($hwnd, ${originalBounds.x}, ${originalBounds.y}, ${originalBounds.w}, ${originalBounds.h}, $true) | Out-Null
`;
if (restoreWindow) {
  await execFileAsync("powershell.exe", ["-NoProfile", "-Command", restoreScript]);
}

const counts = Object.groupBy(result.events, (event) => event.phase);
const summary = Object.fromEntries(Object.entries(counts).map(([phase, events]) => [phase, events.length]));
console.log(JSON.stringify({ ...result, summary }, null, 2));
socket.close();

import { JourneyFailure } from "./wait.mjs";

const CHOICE_KEY = "codeterm:new-session-choice";
const OPEN_EVENT = "codeterm:open-new-session";
const PICKER = '[data-testid="new-session-picker"]';

// In-page waits observe DOM mutations; the timer is only the failure deadline.
function untilScript(checkSource, deadlineMs, label) {
  return `new Promise((resolve, reject) => {
  const check = ${checkSource};
  const first = check();
  if (first) { resolve(first); return; }
  const observer = new MutationObserver(() => {
    const value = check();
    if (value) { observer.disconnect(); clearTimeout(timer); resolve(value); }
  });
  observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true });
  const timer = setTimeout(() => { observer.disconnect(); reject(new Error(${JSON.stringify(`deadline: ${label}`)})); }, ${Number(deadlineMs)});
})`;
}

export function focusedPaneScript(tabId, deadlineMs) {
  const selector = JSON.stringify(`[data-tab-id="${tabId}"]:not([data-testid="tab-item"])`);
  return untilScript(`() => [...document.querySelectorAll(${selector})].some((element) => {
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }) && "focused"`, deadlineMs, `tab ${tabId} visible in the window`);
}

export function openPickerScript({ workspaceId, workspaceCwd, choice }, deadlineMs) {
  const detail = JSON.stringify({ workspaceId, workspaceCwd });
  return `(() => {
  let previous = null;
  try { previous = window.localStorage.getItem(${JSON.stringify(CHOICE_KEY)}); window.localStorage.setItem(${JSON.stringify(CHOICE_KEY)}, ${JSON.stringify(JSON.stringify(choice))}); } catch {}
  if (document.querySelector(${JSON.stringify(PICKER)})) return Promise.reject(new Error("a session picker is already open"));
  window.dispatchEvent(new CustomEvent(${JSON.stringify(OPEN_EVENT)}, { detail: ${detail} }));
  return ${untilScript(`() => document.querySelector(${JSON.stringify(PICKER)}) && "open"`, deadlineMs, "Start a session dialog opened")}
    .then(() => ({ previous }));
})()`;
}

export function startTeamScript(deadlineMs) {
  return `(() => {
  const picker = document.querySelector(${JSON.stringify(PICKER)});
  if (!picker) return Promise.reject(new Error("Start a session dialog is not open"));
  const team = picker.querySelector('[data-testid="picker-mode-orchestration"]');
  if (!team) return Promise.reject(new Error("Team mode is not offered in this dialog"));
  if (team.getAttribute("aria-pressed") !== "true") team.click();
  const alertsBefore = new Set([...picker.querySelectorAll('[role="alert"]')].map((node) => node.textContent));
  return ${untilScript(`() => {
    const confirm = picker.querySelector('[data-testid="picker-confirm"]');
    return team.getAttribute("aria-pressed") === "true" && confirm && !confirm.disabled && confirm;
  }`, deadlineMs, "Team mode selected and Start enabled")}.then((confirm) => {
    confirm.click();
    return ${untilScript(`() => {
      if (!picker.isConnected) return { ok: true };
      const alert = [...picker.querySelectorAll('[role="alert"]')].find((node) => !alertsBefore.has(node.textContent));
      return alert ? { ok: false, alert: alert.textContent } : null;
    }`, deadlineMs, "Start team outcome (dialog closed or alert shown)")};
  });
})()`;
}

export function closePickerScript(previous) {
  return `(() => {
  try {
    if (${JSON.stringify(previous)} === null) window.localStorage.removeItem(${JSON.stringify(CHOICE_KEY)});
    else window.localStorage.setItem(${JSON.stringify(CHOICE_KEY)}, ${JSON.stringify(previous)});
  } catch {}
  document.querySelector('[data-testid="picker-close"]')?.click();
  return true;
})()`;
}

export async function connectPage(dev) {
  const health = await (await fetch(`${dev.url}/api/health`, { signal: AbortSignal.timeout(10_000) })).json();
  const cdpPort = Number(health.instance?.cdp_port);
  const vitePort = health.instance?.vite_port;
  if (!cdpPort) throw new JourneyFailure("DEV exposes no cdp_port; the dialog journey needs the DEV window's CDP endpoint");
  let targets;
  try {
    targets = await (await fetch(`http://127.0.0.1:${cdpPort}/json`, { signal: AbortSignal.timeout(10_000) })).json();
  } catch (error) {
    throw new JourneyFailure(`CDP endpoint ${cdpPort} unreachable: ${error.message}`);
  }
  const page = targets.find((entry) => entry.type === "page" && vitePort && entry.url.includes(`:${vitePort}`))
    ?? targets.find((entry) => entry.type === "page");
  if (!page) throw new JourneyFailure(`no page target on CDP port ${cdpPort}`);
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  const pending = new Map();
  let nextId = 0;
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    pending.get(message.id)?.(message);
    pending.delete(message.id);
  };
  await new Promise((accept, reject) => { socket.onopen = accept; socket.onerror = () => reject(new JourneyFailure("CDP socket failed")); });
  async function evaluate(expression, deadlineMs) {
    const id = ++nextId;
    const reply = new Promise((accept) => pending.set(id, accept));
    socket.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
    const message = await Promise.race([reply, new Promise((_, reject) => {
      AbortSignal.timeout(deadlineMs + 5_000).addEventListener("abort", () => reject(new JourneyFailure("deadline: CDP evaluate")));
    })]);
    const details = message.result?.exceptionDetails;
    if (message.error || details) {
      throw new JourneyFailure(details?.exception?.description ?? details?.text ?? JSON.stringify(message.error));
    }
    return message.result?.result?.value;
  }
  return { evaluate, close: () => socket.close() };
}

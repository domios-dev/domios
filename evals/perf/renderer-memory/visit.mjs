// Visit a tab the way a user does: activate its sidebar item, resolve once it is the
// visible tab, then (with --dwell-lines N) once its terminal parses N more line feeds.
// Prints {tab, readyMs}: click until the tab's terminal is visible with scrollback content.
// Usage: node visit.mjs --cdp 9263 --vite 1460 --tab TAB_ID [--dwell-lines N] [--deadline-ms 30000]
import { connect, evaluate, pageTarget } from "./cdp.mjs";
import { FIND_TERMINALS } from "./terminals.mjs";

const argv = process.argv.slice(2);
const arg = (name, fallback) => (argv.includes(`--${name}`) ? argv[argv.indexOf(`--${name}`) + 1] : fallback);
const tabId = arg("tab");
const deadlineMs = Number(arg("deadline-ms", 30000));
const dwellLines = Number(arg("dwell-lines", 0));

const CLICK = `(() => {
  const button = document.querySelector('[data-testid="tab-item"][data-tab-id=${JSON.stringify(tabId)}] button.sidebar-tab-activation');
  if (!button) return false;
  button.click();
  return true;
})()`;

const WAIT = `new Promise((resolve) => {
  ${FIND_TERMINALS}
  const tabId = ${JSON.stringify(tabId)};
  const wrapper = () => document.querySelector('main > div > [data-tab-id="' + tabId + '"]');
  const visible = () => {
    const layer = wrapper() && wrapper().parentElement;
    return !!layer && layer.style.opacity === "1" && !layer.hasAttribute("inert");
  };
  const startedAt = performance.now();
  let readyMs = null;
  let observer = null;
  const deadline = setTimeout(() => { observer && observer.disconnect(); resolve({ outcome: "deadline", readyMs }); }, ${deadlineMs});
  const done = () => { clearTimeout(deadline); resolve({ outcome: "ok", readyMs }); };
  const terminalReady = () => {
    const el = wrapper() && wrapper().querySelector(".xterm");
    const term = el && findTerminal(el);
    return !!term && term.buffer.active.baseY > 0 && getComputedStyle(el).visibility === "visible";
  };
  const awaitReady = (next) => {
    const finish = () => { readyMs = Math.round(performance.now() - startedAt); next(); };
    if (terminalReady()) return finish();
    const readyObserver = new MutationObserver(() => {
      if (!terminalReady()) return;
      readyObserver.disconnect();
      finish();
    });
    readyObserver.observe(wrapper(), { subtree: true, childList: true, attributes: true, attributeFilter: ["style", "class"] });
  };
  const dwell = () => awaitReady(() => {
    if (${dwellLines} <= 0) return done();
    const onTerminal = (term) => {
      let seen = 0;
      const subscription = term.onLineFeed(() => {
        seen += 1;
        if (seen < ${dwellLines}) return;
        subscription.dispose();
        done();
      });
    };
    const term = terminalsIn(wrapper())[0];
    if (term) return onTerminal(term);
    const mountObserver = new MutationObserver(() => {
      const mounted = terminalsIn(wrapper())[0];
      if (!mounted) return;
      mountObserver.disconnect();
      onTerminal(mounted);
    });
    mountObserver.observe(wrapper(), { subtree: true, childList: true });
  });
  if (visible()) return dwell();
  observer = new MutationObserver(() => {
    if (!visible()) return;
    observer.disconnect();
    dwell();
  });
  observer.observe(document.querySelector("main"), { subtree: true, childList: true, attributes: true, attributeFilter: ["style", "inert"] });
})`;

const target = await pageTarget(Number(arg("cdp")), Number(arg("vite")));
const client = await connect(target.webSocketDebuggerUrl);
try {
  const waiting = evaluate(client, WAIT);
  if (!(await evaluate(client, CLICK))) throw new Error(`sidebar item for tab ${tabId} not found`);
  const { outcome, readyMs } = await waiting;
  if (outcome !== "ok") {
    console.error(`tab ${tabId}: ${outcome} after ${deadlineMs} ms`);
    process.exit(1);
  }
  process.stdout.write(`${JSON.stringify({ tab: tabId, readyMs })}
`);
} finally {
  client.close();
}

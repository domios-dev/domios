import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";

const bootstrap = { origin: "http://127.0.0.1:7697", token: "shell-'token\n$" };
const script = readFileSync("src-tauri/crates/codeterm-service-core/src/startup/frontend_shell_bootstrap.js", "utf8")
  .replace("__CODETERM_SHELL_BOOTSTRAP__", JSON.stringify(bootstrap)).replace("__CODETERM_SHELL_DEBUG__", "false");

function inject(origin: string, frame = false, frontendOrigin?: string, href = origin + "/index.html") {
  const window: { location: { origin: string; href: string }; top?: unknown; __CODETERM_DAEMON__?: unknown } = { location: { origin, href } };
  window.top = frame ? {} : window;
  const scopedScript = frontendOrigin ? readFileSync("src-tauri/crates/codeterm-service-core/src/startup/frontend_shell_bootstrap.js", "utf8")
    .replace("__CODETERM_SHELL_BOOTSTRAP__", JSON.stringify({ ...bootstrap, frontendOrigin })).replace("__CODETERM_SHELL_DEBUG__", "false") : script;
  runInNewContext(scopedScript, { window });
  return window;
}

describe("native shell credential injection", () => {
  it("injects an opaque packaged origin only at the exact fallback document URL", () => {
    expect(inject("null", false, undefined, "tauri://localhost/index.html").__CODETERM_DAEMON__).toEqual(bootstrap);
    for (const href of ["data:text/html,other", "tauri://localhost/other.html", "tauri://foreign/index.html"]) {
      expect(inject("null", false, undefined, href)).not.toHaveProperty("__CODETERM_DAEMON__");
    }
  });
  it("leaves main window creation to the shell in release and development", () => {
    for (const path of ["src-tauri/tauri.conf.json", "src-tauri/tauri.dev.conf.json"]) {
      const config = JSON.parse(readFileSync(path, "utf8"));
      expect(config.app.windows[0].create).toBe(false);
    }
  });
  it("scopes development credentials to the configured Vite origin", () => {
    const frontendOrigin = "http://127.0.0.1:1421";
    expect(inject(frontendOrigin, false, frontendOrigin).__CODETERM_DAEMON__).toEqual({ ...bootstrap, frontendOrigin });
    for (const origin of [bootstrap.origin, "http://127.0.0.1:1420", "http://localhost:1421"]) {
      expect(inject(origin, false, frontendOrigin)).not.toHaveProperty("__CODETERM_DAEMON__");
    }
  });
  it("injects exact credentials before scripts at the daemon and explicit bundled fallback origins", () => {
    for (const origin of [bootstrap.origin, "tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"]) {
      expect(inject(origin).__CODETERM_DAEMON__).toEqual(bootstrap);
    }
  });
  it("withholds credentials from other ports, hosts, recovery pages and subframes", () => {
    for (const origin of ["http://127.0.0.1:7685", "http://localhost:7697", "https://foreign.example", "null"]) {
      expect(inject(origin)).not.toHaveProperty("__CODETERM_DAEMON__");
    }
    expect(inject(bootstrap.origin, true)).not.toHaveProperty("__CODETERM_DAEMON__");
  });
});

it("records document-start failures without placing credentials or exception contents in diagnostic bodies", () => {
  const calls: RequestInit[] = [];
  const handlers: Record<string, (event: unknown) => void> = {};
  const window: Record<string, unknown> = {
    location: { origin: bootstrap.origin, href: bootstrap.origin + "/bundled/index.html" },
    __TAURI_INTERNALS__: {}, addEventListener: (name: string, handler: (event: unknown) => void) => { handlers[name] = handler; },
  };
  window.top = window;
  const debugScript = readFileSync("src-tauri/crates/codeterm-service-core/src/startup/frontend_shell_bootstrap.js", "utf8")
    .replace("__CODETERM_SHELL_BOOTSTRAP__", JSON.stringify(bootstrap)).replace("__CODETERM_SHELL_DEBUG__", "true");
  runInNewContext(debugScript, { window, document: { documentElement: { dataset: {} } },
    fetch: (_url: string, init: RequestInit) => { calls.push(init); return Promise.resolve(); } });
  handlers.error({ error: { name: "ReferenceError", message: bootstrap.token }, filename: "http://127.0.0.1/assets/index.js?token=" + bootstrap.token, lineno: 7, colno: 9 });
  expect(JSON.parse(String(calls[1].body))).toMatchObject({ stage: "script-error", errorName: "ReferenceError", source: "index.js", line: 7, column: 9 });
  handlers.unhandledrejection({ reason: "notification.is_permission_granted not allowed on window main " + bootstrap.token });
  expect(JSON.parse(String(calls[2].body))).toMatchObject({ stage: "promise-error", reason: "native-capability-denied" });
  handlers.unhandledrejection({ reason: bootstrap.token });
  expect(JSON.parse(String(calls[3].body))).toMatchObject({ reason: "rejected-string" });
  handlers.unhandledrejection({ reason: { name: "TypeError", message: bootstrap.token } });
  expect(JSON.parse(String(calls[4].body))).toMatchObject({ errorName: "TypeError", reason: "rejected-value" });
  for (const call of calls) expect(String(call.body)).not.toContain(bootstrap.token);
});


it("records only closed update failure reasons and HTTP status codes", () => {
  const calls: RequestInit[] = [];
  const window: Record<string, unknown> = { location: { origin: bootstrap.origin, href: bootstrap.origin + "/bundled/index.html" }, addEventListener: () => {} };
  window.top = window;
  const debugScript = readFileSync("src-tauri/crates/codeterm-service-core/src/startup/frontend_shell_bootstrap.js", "utf8")
    .replace("__CODETERM_SHELL_BOOTSTRAP__", JSON.stringify(bootstrap)).replace("__CODETERM_SHELL_DEBUG__", "true");
  runInNewContext(debugScript, { window, document: { documentElement: { dataset: {} } },
    fetch: (_url: string, init: RequestInit) => { calls.push(init); return Promise.resolve(); } });
  const diagnostic = window.__CODETERM_SHELL_DIAGNOSTIC__ as (stage: string, error: unknown) => void;
  diagnostic("frontend-initialize-failed", { name: "FrontendUpdateError", reason: "checkpoint-unavailable", httpStatus: 404, message: bootstrap.token });
  expect(JSON.parse(String(calls[1].body))).toMatchObject({ errorName: "FrontendUpdateError", reason: "checkpoint-unavailable", httpStatus: 404 });
  diagnostic("frontend-initialize-failed", { name: bootstrap.token, reason: bootstrap.token, httpStatus: bootstrap.token });
  expect(JSON.parse(String(calls[2].body))).not.toHaveProperty("reason");
  const chat = { scope: "tab:session", edge: "inset", at: 15, state: { inset: 42, clientHeight: 586 } };
  diagnostic("chat-scroll", { chat });
  expect(JSON.parse(String(calls[3].body)).chat).toEqual(chat);
  for (const call of calls) expect(String(call.body)).not.toContain(bootstrap.token);
});

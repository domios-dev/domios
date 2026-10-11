import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";

const bootstrap = { origin: "http://127.0.0.1:7697", token: "shell-'token\n$" };
const script = readFileSync("src-tauri/src/startup/frontend_shell_bootstrap.js", "utf8")
  .replace("__CODETERM_SHELL_BOOTSTRAP__", JSON.stringify(bootstrap));

function inject(origin: string, frame = false, frontendOrigin?: string) {
  const window: { location: { origin: string }; top?: unknown; __CODETERM_DAEMON__?: unknown } = { location: { origin } };
  window.top = frame ? {} : window;
  const scopedScript = frontendOrigin ? readFileSync("src-tauri/src/startup/frontend_shell_bootstrap.js", "utf8")
    .replace("__CODETERM_SHELL_BOOTSTRAP__", JSON.stringify({ ...bootstrap, frontendOrigin })) : script;
  runInNewContext(scopedScript, { window });
  return window;
}

describe("native shell credential injection", () => {
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

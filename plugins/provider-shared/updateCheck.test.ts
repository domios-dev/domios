import { readFileSync } from "node:fs";
import vm from "node:vm";
import { describe, expect, it, vi } from "vitest";
import { checkProviderUpdate, matchesRange, newestSupportedVersion } from "./updateCheck";

const compatibility = {
  supported: ["=1.18.35", ">=1.19.0, <1.20.0"],
  breaking: [{ range: ">=2.0.0, <3.0.0" }],
};

const metadata = { versions: { "1.18.35": {}, "1.19.0": {}, "1.19.8": {}, "1.20.0": {}, "2.0.0": {}, "1.19.9-beta": {} } };
type Await = { __ctAwait__: { job: string; k: string } };

function asyncHost(exec: () => unknown, fetch: () => unknown) {
  const jobs = new Map<string, () => unknown>();
  const awaiting = new Map<string, (result: unknown) => unknown>();
  const host = {
    exec: vi.fn(exec),
    fetch: vi.fn(fetch),
    execStart: vi.fn((opts: string) => { jobs.set("exec", () => host.exec(opts)); return JSON.stringify({ jobId: "exec" }); }),
    fetchStart: vi.fn((opts: string) => { jobs.set("fetch", () => host.fetch(opts)); return JSON.stringify({ jobId: "fetch" }); }),
    awaitJob: <T>(jobId: string, then: (result: unknown) => T): T => {
      awaiting.set(jobId, then as (result: unknown) => unknown);
      return { __ctAwait__: { job: jobId, k: jobId } } as T;
    },
  };
  // Mirrors the host loop: each marker unwinds the call, the job completes outside it, then the continuation resumes.
  const drive = (out: unknown): unknown => {
    while (out && typeof out === "object" && "__ctAwait__" in out) {
      const { job } = (out as Await).__ctAwait__;
      const result = jobs.get(job)!();
      const then = awaiting.get(job)!;
      awaiting.delete(job);
      out = then(result);
    }
    return out;
  };
  return { host, drive };
}

const makeHost = (binary = "opencode", code = 0, body: unknown = metadata) => {
  const { host, drive } = asyncHost(
    () => ({ done: true, code, stdout: `${binary} 1.18.35` }),
    () => ({ status: 200, body: JSON.stringify(body) }),
  );
  return { ...host, drive };
};
const check = (host: ReturnType<typeof makeHost>, binary: string, url: string, compat?: typeof compatibility) =>
  host.drive(checkProviderUpdate(host, binary, url, compat));

describe("provider update selection", () => {
  it("selects the newest supported release below the breaking range", () => {
    expect(newestSupportedVersion(Object.keys(metadata.versions), compatibility)).toBe("1.19.8");
  });
  it("breaking declarations take precedence over supported ranges", () => {
    expect(newestSupportedVersion(["2.0.0"], { supported: [">=1.0.0"], breaking: compatibility.breaking })).toBeNull();
  });
  it("invalid, prerelease and unsupported versions are withheld", () => {
    expect(newestSupportedVersion(["opaque", "1.19.9-beta", "1.20.0", "2.0.0"], compatibility)).toBeNull();
    expect(matchesRange("1.19.0", "invalid")).toBe(false);
    expect(matchesRange("1.20.0", ">=1.19.0, <1.20.0")).toBe(false);
  });
  it("probes the declared binary and returns the exact supported target", () => {
    const host = makeHost();
    expect(check(host, "opencode", "https://opaque/metadata", compatibility)).toEqual({ currentVersion: "1.18.35", latestVersion: "1.19.8" });
    expect(JSON.parse(host.execStart.mock.calls[0][0])).toEqual({ bin: "opencode", args: ["--version"], timeoutMs: 5000 });
  });
  it("missing CLI never fetches metadata", () => {
    const host = makeHost("opencode", 1);
    expect(check(host, "opencode", "https://opaque", compatibility)).toBeNull();
    expect(host.fetchStart).not.toHaveBeenCalled();
  });
  it("current and newer installed versions never downgrade", () => {
    const host = makeHost();
    host.exec.mockReturnValue({ done: true, code: 0, stdout: "opencode 2.0.0" });
    expect(check(host, "opencode", "https://opaque", compatibility)).toBeNull();
  });
  it("latest metadata is used for an unconstrained updater", () => {
    expect(check(makeHost("claude", 0, { version: "2.1.292" }), "claude", "https://opaque")).toEqual({ currentVersion: "1.18.35", latestVersion: "2.1.292" });
  });
  it("malformed metadata and failed HTTP calls fail closed", () => {
    const host = makeHost();
    host.fetch.mockReturnValue({ status: 503, body: "opaque" });
    expect(check(host, "opencode", "https://opaque", compatibility)).toBeNull();
    host.fetch.mockReturnValue({ status: 200, body: "invalid" });
    expect(check(host, "opencode", "https://opaque", compatibility)).toBeNull();
  });
  it("yields before any probe runs and never uses the blocking host calls", () => {
    const host = makeHost();
    const out = checkProviderUpdate(host, "opencode", "https://opaque", compatibility);
    expect(out).toEqual({ __ctAwait__: { job: "exec", k: "exec" } });
    expect(host.exec).not.toHaveBeenCalled();
    expect(host.drive(out)).toEqual({ currentVersion: "1.18.35", latestVersion: "1.19.8" });
    expect(host.execStart).toHaveBeenCalledTimes(1);
    expect(host.fetchStart).toHaveBeenCalledTimes(1);
  });
  it("a refused job start fails closed without waiting", () => {
    const host = makeHost();
    host.execStart.mockReturnValue(JSON.stringify({ error: "exec denied" }));
    expect(checkProviderUpdate(host, "opencode", "https://opaque", compatibility)).toBeNull();
    const fetchRefused = makeHost();
    fetchRefused.fetchStart.mockReturnValue(JSON.stringify({ error: "fetch denied" }));
    expect(check(fetchRefused, "opencode", "https://opaque", compatibility)).toBeNull();
  });
});

interface RuntimePlugin { checkForUpdate?: () => { currentVersion: string; latestVersion: string } | null }

for (const id of ["claude", "opencode", "codex"]) {
  it(`${id} bundle declares the existing update-check capability and manifest permissions`, () => {
    const root = `${process.cwd()}/plugins/provider-${id}`;
    const manifest = JSON.parse(readFileSync(`${root}/plugin.json`, "utf8"));
    const latest = id === "opencode" ? metadata : { version: "2.1.292" };
    const fake = makeHost(id, 0, latest);
    const host = { ...fake, manifestJson: () => JSON.stringify(manifest) };
    const context = { host, module: { exports: {} as { default?: RuntimePlugin } }, exports: {} };
    new vm.Script(readFileSync(`${root}/plugin.js`, "utf8")).runInNewContext(context);
    const plugin = context.module.exports.default;
    expect(typeof plugin?.checkForUpdate).toBe("function");
    const answer = fake.drive(plugin?.checkForUpdate?.()) as { latestVersion: string } | null;
    expect(answer?.latestVersion).toBe(id === "opencode" ? "1.19.8" : "2.1.292");
    expect(manifest.permissions.network.allow).toContain("registry.npmjs.org");
    expect(manifest.permissions.subprocess.allow).toContain(id);
    if (id === "opencode") {
      for (const command of Object.values(manifest.updater)) expect(command).toContain("{version}");
    }
  });
}

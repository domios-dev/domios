import { readFileSync } from "node:fs";
import vm from "node:vm";
import { describe, expect, it, vi } from "vitest";
import { checkProviderUpdate, matchesRange, newestSupportedVersion } from "./updateCheck";

const compatibility = {
  supported: ["=1.18.35", ">=1.19.0, <1.20.0"],
  breaking: [{ range: ">=2.0.0, <3.0.0" }],
};

const metadata = { versions: { "1.18.35": {}, "1.19.0": {}, "1.19.8": {}, "1.20.0": {}, "2.0.0": {}, "1.19.9-beta": {} } };
const makeHost = (binary = "opencode", code = 0, body: unknown = metadata) => ({
  exec: vi.fn((_opts: string) => JSON.stringify({ code, stdout: `${binary} 1.18.35` })),
  fetch: vi.fn((_opts: string) => JSON.stringify({ status: 200, body: JSON.stringify(body) })),
});

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
    expect(checkProviderUpdate(host, "opencode", "https://opaque/metadata", compatibility)).toEqual({ currentVersion: "1.18.35", latestVersion: "1.19.8" });
    expect(JSON.parse(host.exec.mock.calls[0][0])).toEqual({ bin: "opencode", args: ["--version"], timeoutMs: 5000 });
  });
  it("missing CLI never fetches metadata", () => {
    const host = makeHost("opencode", 1);
    expect(checkProviderUpdate(host, "opencode", "https://opaque", compatibility)).toBeNull();
    expect(host.fetch).not.toHaveBeenCalled();
  });
  it("current and newer installed versions never downgrade", () => {
    const host = makeHost();
    host.exec.mockReturnValue(JSON.stringify({ code: 0, stdout: "opencode 2.0.0" }));
    expect(checkProviderUpdate(host, "opencode", "https://opaque", compatibility)).toBeNull();
  });
  it("latest metadata is used for an unconstrained updater", () => {
    expect(checkProviderUpdate(makeHost("claude", 0, { version: "2.1.292" }), "claude", "https://opaque")).toEqual({ currentVersion: "1.18.35", latestVersion: "2.1.292" });
  });
  it("malformed metadata and failed HTTP calls fail closed", () => {
    const host = makeHost();
    host.fetch.mockReturnValue(JSON.stringify({ status: 503, body: "opaque" }));
    expect(checkProviderUpdate(host, "opencode", "https://opaque", compatibility)).toBeNull();
    host.fetch.mockReturnValue(JSON.stringify({ status: 200, body: "invalid" }));
    expect(checkProviderUpdate(host, "opencode", "https://opaque", compatibility)).toBeNull();
  });
});

interface RuntimePlugin { checkForUpdate?: () => { currentVersion: string; latestVersion: string } | null }

for (const id of ["claude", "opencode", "codex"]) {
  it(`${id} bundle declares the existing update-check capability and manifest permissions`, () => {
    const root = `${process.cwd()}/plugins/provider-${id}`;
    const manifest = JSON.parse(readFileSync(`${root}/plugin.json`, "utf8"));
    const latest = id === "opencode" ? metadata : { version: "2.1.292" };
    const host = { ...makeHost(id, 0, latest), manifestJson: () => JSON.stringify(manifest) };
    const context = { host, module: { exports: {} as { default?: RuntimePlugin } }, exports: {} };
    new vm.Script(readFileSync(`${root}/plugin.js`, "utf8")).runInNewContext(context);
    const plugin = context.module.exports.default;
    expect(typeof plugin?.checkForUpdate).toBe("function");
    expect(plugin?.checkForUpdate?.()?.latestVersion).toBe(id === "opencode" ? "1.19.8" : "2.1.292");
    expect(manifest.permissions.network.allow).toContain("registry.npmjs.org");
    expect(manifest.permissions.subprocess.allow).toContain(id);
    if (id === "opencode") {
      for (const command of Object.values(manifest.updater)) expect(command).toContain("{version}");
    }
  });
}

import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { scanTransportSource, transportViolationKey } from "./transportParityScan";
import allowlist from "./transportParityAllowlist.json";

const mode: "report" | "enforce" = "report";
function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (["node_modules", "dist", "target", "generated"].includes(entry.name)) return [];
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.[cm]?[jt]sx?$/.test(entry.name) && !/\.(test|spec|testdata)\./.test(entry.name) ? [path] : [];
  });
}

describe("transportParity", () => {
  it("detects aliases, dynamic commands, reexports and dynamic Tauri imports", () => {
    const source = `import { invoke as call } from './bridge';
      call('get_config'); call(schema.command);
      export { getCurrentWindow } from '@tauri-apps/api/window';
      const native = import('@tauri-apps/api/core');`;
    expect(scanTransportSource("fixture.ts", source).map(({ kind, target }) => ({ kind, target }))).toEqual([
      { kind: "invoke", target: "get_config" }, { kind: "invoke", target: "<dynamic>" },
      { kind: "tauri-import", target: "@tauri-apps/api/window" }, { kind: "tauri-import", target: "@tauri-apps/api/core" },
    ]);
  });
  it("ignores comments and string examples", () => {
    expect(scanTransportSource("fixture.ts", `// import '@tauri-apps/api/core';\nconst example = "invoke('get_config')";`)).toEqual([]);
  });
  it("reports the remaining transport bypass inventory in src and packages", () => {
    const sites = ["src", "packages"].flatMap(sourceFiles).flatMap((path) => {
      const file = relative(process.cwd(), path).replaceAll("\\", "/");
      return file === "src/lib/nativeBridge.ts" ? [] : scanTransportSource(file, readFileSync(path, "utf8"));
    });
    const allowed = new Set(allowlist);
    const newSites = sites.filter((site) => !allowed.has(transportViolationKey(site)));
    console.info(`[transportParity mode=${mode}] ${sites.length} remaining Tauri import / invoke sites`);
    for (const site of sites) console.info(`${site.file}:${site.line} ${site.kind} ${site.target}${allowed.has(transportViolationKey(site)) ? " [allowlisted]" : " [new]"}`);
    if (String(mode) === "enforce") expect(sites).toEqual([]);
    else expect(newSites.map(transportViolationKey)).toEqual([]);
    expect(allowlist.length).toBe(new Set(allowlist).size);
  });
});

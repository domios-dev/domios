import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { NATIVE_COMMANDS } from "./nativeCommands";
import { scanTransportSource } from "./transportParityScan";

const nativeSource = readFileSync("src/lib/nativeBridge.ts", "utf8");
describe("native capability confinement", () => {
  it("exposes exactly the declared native commands through Rust IPC", () => {
    const source = readFileSync("src-tauri/src/command_registration.rs", "utf8");
    const registered = [...source.matchAll(/(?:commands|clipboard_files)::(?:[a-z_]+::)*([a-z_]+),/g)].map(match => match[1]);
    const calls = scanTransportSource("nativeBridge.ts", nativeSource).filter(site => site.kind === "invoke").map(site => site.target);
    expect([...new Set(calls)].sort()).toEqual(Object.keys(NATIVE_COMMANDS).sort());
    expect(registered.sort()).toEqual(Object.keys(NATIVE_COMMANDS).sort());
  });
  it("confines native plugins to the declared bridge members", () => {
    const modules = scanTransportSource("nativeBridge.ts", nativeSource).filter(site => site.kind === "tauri-import").map(site => site.target);
    expect(modules.sort()).toEqual([
      "@tauri-apps/api/core", "@tauri-apps/api/event", "@tauri-apps/api/tray",
      "@tauri-apps/api/webview", "@tauri-apps/api/window", "@tauri-apps/plugin-dialog",
    ]);
    const capability = JSON.parse(readFileSync("src-tauri/capabilities/default.json", "utf8"));
    expect(capability.windows).toEqual(["main"]);
    expect(capability.remote).toBeUndefined();
    expect(capability.permissions.sort()).toEqual([
      "core:event:allow-listen", "core:event:allow-unlisten", "core:tray:allow-get-by-id", "core:tray:allow-set-visible",
      "core:window:allow-close", "core:window:allow-minimize", "core:window:allow-set-focus", "core:window:allow-show",
      "core:window:allow-start-dragging", "core:window:allow-toggle-maximize", "dialog:allow-open", "dialog:allow-save",
    ]);
    const schema = readFileSync("src-tauri/gen/schemas/desktop-schema.json", "utf8");
    for (const permission of capability.permissions) expect(schema).toContain(`"const": "${permission}"`);
  });
});

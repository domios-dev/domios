import type { BlueprintDocument, BlueprintPluginNode } from "../../../lib/blueprintApi";
export const document: BlueprintDocument = {
  schema_version: 2, kind: "pipeline", name: "plugin-fixture",
  trigger: { type: "event", kind: { lit: "fixture" } },
  steps: [
    { id: "read", uses: "fixture.read", with: { url: { lit: "https://example.com" }, limit: { lit: 5 } }, routes: { read: "use", busy: "@stop" } },
    { id: "use", code: { lang: "python", src: "def main(value): return value" }, bind: { in: "read.out.text" } },
    { id: "gone", name: "Saved page reader", uses: "missing.step", routes: { failed: "@stop", done: "@stop" } },
  ],
  result: { to: "owner" },
};
export const pluginNodes: Record<string, BlueprintPluginNode> = {
  "fixture.read": {
    available: true, title: "Fixture: read page", plugin: "fixture", description: "Reads one page",
    inputs: { url: { kind: "text", minItems: 0, maxItems: 1 } },
    outputs: { text: { kind: "text", minItems: 1, maxItems: 1 }, shot: { kind: "image", minItems: 0, maxItems: 1 } },
    outcomes: ["read", "busy"],
  },
  "missing.step": { available: false, plugin: "Archive" },
};

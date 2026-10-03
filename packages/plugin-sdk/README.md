# @codeterm/plugin-sdk

Type-only SDK for authoring [Domios](https://github.com/rollacode/codeterm)
plugin **logic** in TypeScript.

A Domios plugin's logic half runs in a confined QuickJS VM and talks to the
host only through the injected `host.*` API. This package gives you the typed
contracts for that surface — the `host` global, and the capability interfaces a
plugin can implement — with zero runtime weight (it's purely types; your bundler
erases the imports).

```ts
import type { PluginModule } from "@codeterm/plugin-sdk";

const plugin: PluginModule = {
  statusBubble(ctx) {
    if (!ctx?.cwd) return null;
    const r = JSON.parse(host.exec(JSON.stringify({ bin: "git", args: ["-C", ctx.cwd, "rev-parse", "--abbrev-ref", "HEAD"] })));
    return r.code === 0 ? { label: r.stdout.trim(), tone: "default", action: "openPanel:view:my-plugin" } : null;
  },
};

export default plugin;
```

Build it to the QuickJS-compatible `plugin.js` with Domios's `build-plugin.mjs`
(esbuild, CJS, `module.exports.default`). QuickJS has no `console`/`fetch`/timers —
stay on the `host.*` surface. See the Domios plugin authoring guide for details.

Exports: `Host`, `HostFs`, `HostShell`, `HostPath`, `DirEntry`, `ExecOpts`,
`ExecResult`, `FetchOpts`, `FetchResult`, and the capability contracts
(`PluginModule`, `StatusBubble`, `GlanceView`, `ViewNode`, `ViewCall`,
`AgentProvider`, `SecretBackend`, `Transcriber`, `DecisionModelCapability`,
`DecisionModelInfo`, `DecisionRequest`, `DecisionAnswer`, `DecisionPrimitives`,
`DecisionQuestion`, …).

## Decision model capability

Decision adapters may expose `selectModel(id): boolean` to apply a catalogue model immediately (modelId must report the selected id), and `metadata()` for display_name, help_url, server_address and settings_url. Metadata must never contain credentials. Without selectModel, the host only accepts the model reported by modelId.

Declare `"decisionModel": true` in `plugin.json` and export `primitives()`,
`models()`, and `decide(request)`. `models()` returns entries with `id` and
`display_name`; the host uses them for the supporter model picker. A request
carries `state`, `instructions`, and a tagged
`question` (`choice`, `noul`, or `score`). Answers use the same tagged JSON
shapes as the Rust decision types. Use `host.fetch.async` for network calls so
the host can serve other candidate decisions while a request is in flight.

`decideQuestions(request)` is optional; without it the host asks questions
sequentially. Event observation adapters must also export
`supportsQuestionBatch() { return true; }` and answer the complete named question
set using at most one physical HTTP request. Merely exporting `decideQuestions` does not
declare batching. The host counts actual `host.fetch` transport attempts,
including async and stream calls, and enforces the event's request deadline and
one-request limit. A local decision with no HTTP call spends no request budget.

Bare `true` means the token is optional and remains readable by older hosts.
For a required or token-free adapter, use
`"decisionModel": { "tokenMode": "required" }` or
`"decisionModel": { "tokenMode": "none" }`; the object form requires
`minCodeterm: "1.12.4"`. An optional object declaration is also accepted.
Domios stores tokens host-side and attaches a bearer header only for an
allowed `host.fetch`, `host.fetch.async`, or `host.fetch.stream` request on
1.12.4 hosts. This uses the existing `hostApi: 2` contract and needs no plugin
opt-in. Plugin code never receives the token; older hosts do not add the header.

```ts
import type { DecisionModelCapability, PluginModule } from "@codeterm/plugin-sdk";

const decisionModel: DecisionModelCapability = {
  primitives: () => ({ choice: true, noul: true, score: true }),
  models: () => [{ id: "loaded-model", display_name: "Loaded model" }],
  decide(request) {
    const encoded = JSON.stringify(request);
    return host.fetch.async(
      {
        url: "http://127.0.0.1:1234/decision",
        method: "POST",
        headers: { "content-type": "application/json" },
        body: encoded,
      },
      (result) => {
        if (result.error || !result.body) throw new Error("decision request failed");
        return JSON.parse(result.body) as ReturnType<DecisionModelCapability["decide"]>;
      },
    );
  },
};

const plugin: PluginModule = { ...decisionModel };
export default plugin;
```

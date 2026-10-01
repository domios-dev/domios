# @codeterm/plugin-sdk

Type-only SDK for authoring [CodeTerm](https://github.com/rollacode/codeterm)
plugin **logic** in TypeScript.

A CodeTerm plugin's logic half runs in a confined QuickJS VM and talks to the
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

Build it to the QuickJS-compatible `plugin.js` with CodeTerm's `build-plugin.mjs`
(esbuild, CJS, `module.exports.default`). QuickJS has no `console`/`fetch`/timers —
stay on the `host.*` surface. See the CodeTerm plugin authoring guide for details.

Exports: `Host`, `HostFs`, `HostShell`, `HostPath`, `DirEntry`, `ExecOpts`,
`ExecResult`, `FetchOpts`, `FetchResult`, and the capability contracts
(`PluginModule`, `StatusBubble`, `GlanceView`, `ViewNode`, `ViewCall`,
`AgentProvider`, `SecretBackend`, `Transcriber`, `DecisionModelCapability`,
`DecisionRequest`, `DecisionAnswer`, `DecisionPrimitives`, `DecisionQuestion`, …).

## Decision model capability

Declare `"decisionModel": true` in `plugin.json` and export `primitives()` plus
`decide(request)`. A request carries `state`, `instructions`, and a tagged
`question` (`choice`, `noul`, or `score`). Answers use the same tagged JSON
shapes as the Rust decision types. Use `host.fetch.async` for network calls so
the host can serve other candidate decisions while a request is in flight.

```ts
import type { DecisionModelCapability, PluginModule } from "@codeterm/plugin-sdk";

const decisionModel: DecisionModelCapability = {
  primitives: () => ({ choice: true, noul: true, score: true }),
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

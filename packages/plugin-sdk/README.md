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
`AgentProvider`, `SecretBackend`, `Transcriber`, …).

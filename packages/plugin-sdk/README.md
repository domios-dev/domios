# @codeterm/plugin-sdk

SDK for authoring [Domios](https://github.com/domios-dev/domios)
plugin **logic** in TypeScript.

A Domios plugin's logic half runs in a confined QuickJS VM and talks to the
host only through the injected `host.*` API. This package gives you the typed
contracts for that surface — the `host` global, and the capability interfaces a
plugin can implement. Type imports have zero runtime weight. The optional pure
diff helpers are bundled only when imported as values.

For plugin-owned storage, use `host.dataDir()`. `host.fs.expandHome("~/.domios")`
and paths beneath `~/.domios/` resolve to the running instance's data directory,
including development and named instances. Other `~/` paths use the user's home;
the former product directory has no special mapping. Filesystem grants still apply after expansion.
The SDK package name and `CODETERM_*` host environment variables remain the
public technical contract; they do not select a legacy data root.

`capabilities.localRecordingTrigger` declares a USB vendor/product and HID
usage-page/usage tuple. It permits a host-owned `receiverTarget` tab-menu item
without a webhook receiver. `local_recording` needs an explicit persisted user
grant; requesting it in a manifest, system trust, and dev mode do not grant it.
`host.localRecording.status()` and `setEnabled(boolean)` address only the calling
plugin. Enabling arms the trigger without recording. With the same declaration
and user grant, `listInputDevices()` exposes the host's existing input inventory,
`selectDevices({inputId, collectionId, microphoneId})` selects and persists this
plugin's identities on this machine, and `savedSelection()` reads only its own
saved choice. The plugin owns device choice; the host validates the declared input
at subscription setup and freezes an authenticated ready recipient at
START, transcribes with the configured voice stage, and admits ordinary tracked
voice through the existing permission/text pipeline and client token.

Plugins may export synchronous `onPermissionChanged(event)` to observe an
approved grant after it is persisted and applied. The event is
`{ permission, allowed: true, source: "install" | "action" | "settings" }`.
Install and update consent uses `source: "install"`; a plugin can initialize
itself there. An action-time grant uses `source: "action"` and the original action
still retries once, so the callback must not separately activate that action.
The callback receives only its own allowed permission; Not now and revocation
emit no approval event. This callback grants no additional host access and may
return `{ error }` if initialization fails. A missing export is a no-op.

An explicit closed recipient stays stale until the owner clears or replaces it.
General resolves only at START when there is no explicit selection. Cancellation,
device loss, grant revocation on reload, and unload invalidate subscription and
capture generations. Linux and web capture are unsupported. The SDK exposes no
raw HID, audio, capture lease, transcript commit, or arbitrary
recipient sender. Windows/macOS adapter and desktop recorder implementation belong
to the host; the declaration alone does not prove hardware capture or delivery.
Input inventory currently contains Windows HID receivers, with the native device
path as identity and fallback label. macOS/Linux inventory returns an empty list.
Microphone IDs are supplied by desktop media-device enumeration. Generic audio
inventory and device-change subscriptions require further host support.

```ts
import type { PluginModule } from "@codeterm/plugin-sdk";

const plugin: PluginModule = {
  statusBubble(ctx) {
    if (!ctx?.cwd) return null;
    return host.exec.async({ bin: "git", args: ["-C", ctx.cwd, "rev-parse", "--abbrev-ref", "HEAD"], timeoutMs: 4000 }, (r) =>
      r.code === 0 && !r.error && !r.truncated
        ? { label: (r.stdout || "").trim(), tone: "default", action: "openPanel:view:my-plugin" }
        : null);
  },
};

export default plugin;
```

Build it to the QuickJS-compatible `plugin.js` with Domios's `build-plugin.mjs`
(esbuild, CJS, `module.exports.default`). QuickJS has no `console`/`fetch`/timers —
stay on the `host.*` surface. See the Domios plugin authoring guide for details.

Return `host.exec.async(opts, continuation)` from a hook to release the VM while
the subprocess runs. It uses the same binary allowlist, host identity environment,
and detached-log write policy as synchronous `host.exec`; it grants no additional
filesystem, SQLite, or environment access. Its event wait has a five-second failure
deadline. Each output stream retains at most 128 KiB, with `truncated: true` when
bytes were discarded. Keep `timeoutMs` below the event deadline and retain the last
successful display on errors or truncation. Hooks return continuations, not Promises.

The host snapshots callable exports when the bundle evaluates; methods added later are invisible until a bundle reload includes them.

A plugin-owned chat backend confirms `closeSession` with
`{ sessionId, processExited: true }` only after that exact session's owned
processes have exited. Return `host.awaitJob(jobId, continuation)` when closure
needs asynchronous work. Missing, failed, foreign-session or incomplete receipts
refuse task respawn. A host-owned `agentRuntime` uses the host's pinned process
exit observation instead. No additional subprocess or sandbox authority is granted.

Exports: `Host`, `HostFs`, `HostShell`, `HostPath`, `DirEntry`, `ExecOpts`,
`ExecResult`, `FetchOpts`, `FetchResult`, and the capability contracts
(`PluginModule`, `StatusBubble`, `GlanceView`, `ViewNode`, `ViewCall`,
`AgentProvider`, `SecretBackend`, `Transcriber`, `Converter`,
`GraphNodesManifestCapability`, `EventSourceDeclaration`, `NodeSchema`,
`DecisionModelCapability`, `DecisionModelInfo`, `DecisionRequest`,
`DecisionAnswer`, `DecisionPrimitives`, `DecisionQuestion`, …).

`recordedTextDiff(path, oldText, newText)` maps explicit old/new evidence into
FileDiff records. Both strings are required. Ranges are relative to the recorded
fragment and marked `truncated`; missing file context or creation evidence is
never inferred. CR characters are preserved. `recordedPatchDiff(path, patch,
kind)` retains unified hunk coordinates and rejects incomplete hunk counts.

## Graph nodes and the converter capability

Declare media conversions and event sources under `capabilities.graphNodes` and
export `convert(request)` for the conversion edges:

```json
{
  "capabilities": {
    "graphNodes": {
      "converters": [{ "from": "audio", "to": "text" }],
      "eventSources": [{
        "kind": "telegram.message",
        "title": "Telegram: new message",
        "params":  { "type": "object", "properties": { "chat_id": { "type": "string", "maxLength": 128 } }, "required": ["chat_id"] },
        "payload": { "type": "object", "properties": { "text": { "type": "string", "maxLength": 16384 } }, "required": ["text"] }
      }]
    }
  }
}
```

`convert(request)` receives `{ inputPath, from, to, outputDir, language?,
deadlineMs }` and returns `{ text }`, `{ path }` (a file inside `outputDir`),
or `{ error }`. The host chains conversions across plugins from the declared
edges alone, so a plan never depends on which plugin serves a step.

`params`/`payload` use a closed schema subset: an object of scalar
(`string` | `integer` | `number` | `boolean`) or one-level scalar-array
properties with optional `description`, `enum`, `maxLength` (<= 65536) and
`maxItems` (<= 256); at most 32 properties. `$ref`, combinators
(`oneOf`/`anyOf`), `patternProperties` and nested objects are refused at parse.

## Blueprint step declarations

`GraphNodesManifestCapability.steps` accepts up to 32 `BlueprintStepDescriptor`
records. A plugin may declare specialized nodes and a general node whose model
and parameters are selected by its own bounded schema. The host never knows
supplier models or endpoints.

```ts
import type { BlueprintStepDescriptor } from "@codeterm/plugin-sdk";

const generate: BlueprintStepDescriptor = {
  contractVersion: 1, revision: 1, id: "fixture.generate",
  title: "Generate", description: "Generate an output from a prompt",
  params: { type: "object", properties: {
    model: { type: "string", enum: ["small", "large"], maxLength: 16 },
  }, required: ["model"] },
  inputs: {
    prompt: { kind: "text", minItems: 1, maxItems: 1 },
    references: { kind: "image", minItems: 0, maxItems: 4 },
  },
  outputs: { image: { kind: "image", minItems: 1, maxItems: 1 } },
  outcomes: ["done", "failed"],
  lifecycle: { reconcile: true, cancel: true },
  effects: { sideEffects: true },
};
```

Export `blueprintSubmit` and the declared optional `blueprintReconcile` and
`blueprintCancel` methods from `BlueprintSteps`. Requests carry host-owned
operation identity and attribution. An uncertain submit is never replayed;
reconciliation and cancellation refer to the exact persisted job. Invalid
responses retain a valid returned job identity for later observation.

The host reserves `unknown` and `canceled`; other bounded declared outcomes route
the graph. Parameters and JSON ports use closed bounded schemas. Text, JSON,
file, image, audio and video ports declare cardinality. Media outputs use
`{kind: "media_source", id, media}` for an existing plugin-owned artifact.
`mediaFile` resolves it relative to the plugin data directory; generic files may
use `application/octet-stream`. Host handles are inputs only and require the
receiving step's explicit grant. UI frame tokens grant no run-input access.

YAML authors write `uses: fixture.generate` and `with:`. Apply pins the installed
descriptor revision. A built-in node bound to `read.out.text` receives the plain
string, JSON value or confined file path; multi-item ports supply arrays.
Commands receive that value on stdin, with JSON values encoded as JSON.
Plugin-to-plugin bindings retain typed run-media handles. Durable held runs
reconcile their exact job through the Blueprint monitor and resume at the cursor.

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

Decision adapters that provide calibrated choice probabilities may export
`supportsCalibratedChoiceConfidence() { return true; }`.

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

### Selected receiver status

`capabilities.receiverStatusQuery` declares one immutable HID status request (2–64bytes), application-collection usage and reportID. `host.localRecording.statusReport()` exchanges only that declaration with a sibling collection belonging to the exact selected receiver; persisted `local_recording` grant and current selection/availability are checked before and after IO. No device path or request argument is exposed. The plugin owns decoding, connection/battery semantics and `statusBubble`; unsupported platforms or unavailable status return a refusal. `TabContext.tabId` identifies the host-supplied bubble recipient.

`localRecordingTrigger.button` supplies the selected interface/collection, report schema, approved singleton button usages, press usage and duplicate window. Native adapters validate that declared shape instead of branching on manufacturer identity. A missing native mapping remains unavailable.


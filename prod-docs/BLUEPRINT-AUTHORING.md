# Blueprint authoring

Use this one cookbook for every model. Start with `domios blueprint new --intent "<task purpose>" --json`; it returns up to three compact scoped recall cards and a native-v2 scaffold carrying the exact intent. `--limit N` accepts 1..20. Keep the scaffold even when recall has no fit or is degraded. Cards report `mem:` or `catalog:` provenance, score, WHY, typed signature, node kinds and enrichment state. Open a selected body explicitly with `domios blueprint show <id> --json`; no card contains executable bodies.

v2 essentials: schema_version: 2; name; inputs mapping of typed declarations; types mapping of JSON schemas; steps list with id. A code step has does, code: {lang: python, src: ...}, in/out schemas and bind: {in: prior.out}. Python main(value) returns JSON. `lang` is `python` or `ts` (not `typescript`; `shell` is not runnable, use a fixed `run` step). Code limits go under the code block: `code: {lang: python, src: ..., limits: {wall_ms: 3000, output_bytes: 32768}}`. Code strings may not contain the whole words `domios` or `codeterm`, or names starting `DOMIOS_` or `CODETERM_` (R10). `result.to: owner` delivers to the Team Orchestrator's inbox for a Team Blueprint. The `domios` on PATH can be an older build without `blueprint new --intent`, `check --explain` or `--fix`; confirm with `domios blueprint --help` and use the running instance's CLI. Routes done/failed can target a later id, next or stop. Run `domios blueprint new --intent "<task purpose>" --json` first. Reuse a card by typed call when its purpose and complete input/result signature fit; otherwise author from scratch. A high score alone does not prove fit.

`bind.in` names an earlier step's output, never a literal value or workflow input. Generic fabric example (steps only):
```yaml
- id: swatch
  does: Supply a fabric label
  code: {lang: python, src: 'def main(value): return "linen"'}
  out: {type: string}
- id: caption
  does: Capitalize the fabric label
  in: {type: string}
  bind: {in: swatch.out}
  code: {lang: python, src: 'def main(value): return value.upper()'}
  out: {type: string}
```

Use `call: {ref: "mem:<id>", with: {args: {lit: <typed-value>}}}` with the exact argument names/schemas from the card; `catalog:<id>` uses the same form. Arguments are structural `{lit: ...}`, `{input: name}` or `{bind: prior.out.field}` values. Declare the call node's `out` to match the returned result. Copy/adapt a body only when the required structure must change. Irrelevant candidates are a no-fit: do not call them.

Literal array arguments may contain differing values and optional fields as long as every item satisfies the callee's `items` schema. Empty arrays are accepted too. For `catalog:checks-fan-in`, use `with: {args: {lit: {checks: []}}}` for no checks, or supply check objects with their own `name`, boolean `ok` and `detail` values.

Run `domios blueprint check authored.yaml --json` and require `ok: true`. For calls, live scoped checking seals the exact callee snapshot and returns `definition` with real `call.pin: {id, hash}` and `dependencies`. Write that returned definition to `sealed.json`, then apply that file with `domios blueprint apply sealed.json`. Never invent a hash or discard the returned dependencies. A missing pin revision refuses; a current body changing does not silently replace a retained snapshot. After execution, inspect `domios blueprint show <id> --json` and `domios blueprint log <id> --json` for the retained pin, expanded callee and actual outcome. An R13 `exact` or `structural_twin` refusal names the existing Blueprint it duplicates; graphs whose code, commands or literals differ are not twins. Write `code.file` paths with forward slashes.

Native documents also declare `kind: pipeline` or `kind: watch`. A callable has no trigger and declares `result: {type: <schema>, from: <node>.out}`; `type` holds one schema object, so an integer result is `result: {type: {type: integer}, from: add.out}`. A single named input passes its value to the first callee node; several inputs pass a named object. For a single object contract use `inputs: {args: <object-schema>}` and matching first-node `in`. A runnable pipeline needs a trigger; a one-shot text input uses `trigger: {type: input, kind: {lit: text}, source: {lit: {text: "<input text>"}}}`. The text arrives as a string; it is read as JSON only when the first node's `in` schema rejects a string (`in: {type: object}` with `{"n":7}` delivers the object, `in: {type: string}` with `7` delivers `"7"`). A mismatch stops with `contract_violation` naming the expected schema and the received value. Each code or command step runs with the user's own permissions, like an agent in a tab, without Domios credentials in its environment; foreign Blueprints are gated by folder trust (1.15.4). A refusal never counts as execution.

A fixed command `run: {lit: "<author-fixed command>"}` is a shell command line, not argv: macOS/Linux use `/bin/sh -c`; Windows uses `powershell.exe -NoLogo -NoProfile -NonInteractive -Command`. Shell quoting and expansion apply, including `$(...)` inside double quotes; `lit` does not disable expansion. With `in` and `bind: {in: prior.out}`, the command receives the JSON stdin envelope `{media, schema_id, value, text}`. Runtime data stays in `value` and never becomes shell text. Full node/format reference: `domios docs BLUEPRINTS`; named recipes: `domios docs BLUEPRINT-RECIPES`. Save reusable triggerless files with `domios blueprint keep --file callable.yaml --when "<when to use it>"` from the Team Orchestrator.

<!-- revision: 5 -->

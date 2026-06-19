# chatBackend / LM Studio — robust tool-calls + rendering + per-model prompts/presets + UX

> Owner: Dante (manager). Brief from Andrey 2026-06-19 (+ screenshot of the current
> strict lmstudio prompt). Theme: small local models can't hold a strict tool-call
> format — meet them where they are. Make the HOST tolerant + the prompt simpler +
> the preset/model system flexible + the UX nicer. Branch off `plugins-rethink`.

**Core shift:** today the lmstudio prompt forces ONE strict format
(` ```codeterm-tool ` fenced JSON) and the plugin (QuickJS/JS) does fragile
parsing — weak models emit native syntax, wrap in ``` of various langs, add prose
junk, and the call leaks as raw text. Flip it: a STRONG, tolerant tool-call
extractor in **Rust** (host-side, generic) that understands the many shapes weak
models actually produce, strips the junk, and hands back a structured call. Rust
is far better at this than QuickJS. The plugin keeps owning the tool SET + the
ReAct loop; it just delegates the messy extraction to the host.

## R1 — Robust tool-call extractor (Rust, GENERIC host utility) — branch `cbr-r1-parser`
**Role:** Rust backend. **Boundary:** generic — it does NOT know "lmstudio"; the
PLUGIN passes its own tool schema (names + arg shapes). New host API
`host.toolcall.parse(rawText, schema) -> { tool, args, confidence, span } | null`
(pure text→struct; no FS/net/secrets → low confinement risk; justify in the
host-API audit). Native runtime + a QuickJS binding; parity-tested.

Multi-strategy extractor (fallback chain, highest-confidence wins):
1. **De-fence**: strip ``` code fences of ANY lang tag (```codeterm-tool, ```json,
   ```tool, ```sh, bare ```), and inline-code backticks around a JSON object.
2. **Native syntaxes**: recognize and unwrap the shapes weak models emit —
   `<|tool_call|>…<|/tool_call|>` (gemma), `call:[ns:]NAME{json}`, OpenAI-style
   `{"tool_calls":[{"function":{"name","arguments"}}]}`, `.tool_call` arrays,
   bare `{"tool":…,"args":…}`, and a NAME-then-JSON line.
3. **JSON repair (best-effort)**: tolerate trailing prose, single quotes, unquoted
   keys, smart-quotes, a missing closing brace — repair conservatively, never
   guess values.
4. **Normalize aliases**: `command`→`cmd`, `file`/`filepath`→`path`, etc. (alias
   map provided by the plugin schema, not hardcoded in core).
5. **Validate** against the plugin schema: tool name ∈ the curated set, args shape
   matches; reject (return null) otherwise — do NOT execute an unknown/invented
   tool. Confidence reflects which strategy matched + how much repair was needed.
- TDD: a GOLDEN CORPUS of real messy outputs (fenced, native gemma, OpenAI-style,
  bare, prose-wrapped, alias'd, broken-JSON) → correct {tool,args}; a NEGATIVE
  corpus (plain prose, a code block that ISN'T a tool call, a half-written call) →
  null (no false-positive execution). Grow the corpus on every miss.
- Gate: cargo build/test + hook clippy (src + cli) + parity_test (native vs JS).

## R2 — Structured tool-call rendering (frontend) — branch `cbr-r2-render`
**Role:** React/TS (chat-ui). Once R1 yields a structured call, render it as a
per-tool STYLED card (not raw text): exec → a terminal-styled command chip;
read_file/write_file → a file chip with the path; codeterm → CLI verb; mem_search
→ query; spawn_agent → provider/task. Style by tool + by phase (pending → running
→ result) so the user sees what the model is doing, pleasantly. Reuse the existing
chat-ui tool-call card components where possible. TDD: each tool renders its card;
unknown → graceful fallback. Gate: tsc + test:unit.

## R3 — Simpler, example-led lmstudio system prompt (+ per-model variants) — branch `cbr-r3-prompt`
**Role:** plugin (lmstudio). Rewrite the system prompt to be SHORT, concrete, and
example-led (small models learn from examples, not rules). Lead with 1-2 worked
tool-call examples; keep the curated tool table terse; drop the heavy "do NOT use
native syntax" warnings (R1 now tolerates them, so the prompt doesn't have to
fight the model). Add a per-model prompt VARIANT hook so the active prompt can be
tuned to the running model (see R4). Verify live against the user's gemma in LM
Studio. Gate: plugin build/test + a live chat round-trip executing a tool.

## R4 — Flexible preset ↔ model binding — branch `cbr-r4-presets`
**Role:** plugin + small backend/config. A preset = {id, name, systemPrompt,
model?, params?}. Make the binding flexible:
- A preset MAY bind to a specific model id. On session init: if the chosen model
  has a bound preset → use it; else fall back to the default preset; or the user
  explicitly invokes a named preset at init.
- Per-model system-prompt override: a model can carry its own tuned prompt
  (R3 variant) so a weak model gets a simpler prompt automatically.
- Stored plugin-side (config.plugins.<id> / preset store) — core stays generic
  (it already gets models/presets via capabilities). No hardcoded lmstudio in core.
- TDD: bound-model resolves its preset; unbound → default; init-with-named-preset.
- Gate: plugin tsc/test + (if a backend seam is touched) cargo + hook clippy.

## R5 — UX pass (frontend-design skill) — branch `cbr-r5-ux`
**Role:** React/TS, **use the `frontend-design` skill**. Three pieces:
1. **Model-picker bubble relocation** — move the chatBackend `ChatModelBubble`
   (CB-P3) to the SAME spot the terminal/model bubble sits in the other
   terminal-subsystem chats (consistent, clickable). Find that seam
   (ChatComposer / the PTY-agent model bubble location) and mirror it.
2. **Preset manager** — the D1-D4 preset UX (supersedes those): list/create/edit
   presets (system prompt + model + params), bind a preset to a model, pick the
   active preset. Schema-rendered, polished (this replaces the raw info/text
   schema rows noted in the earlier D-tracks).
3. **System-prompt card** polish — the active prompt remains observable as the
   special card (S3/H1), now nicely styled.
- Gate: tsc + test:unit + a design self-review per the skill.

## R6 — Author/refine a pane's system prompt via another pane — branch `cbr-r6-authoring`
**Role:** plugin + small UI. A flexible authoring flow: from a chatBackend pane,
hand off "tune this pane's system prompt for model X" to another agent pane (or an
inline action) that reads the current prompt + model, drafts a tuned prompt, and
writes it back into this pane's preset/binding. So a user can iteratively improve
the per-model prompt without hand-editing JSON. Keep it plugin-mediated. TDD: the
authoring round-trip updates the bound prompt; init then uses it. Gate: tsc/test
(+ cargo/hook clippy if a backend seam is touched).

## Sequencing / notes
- **R1 first** (it unblocks R2's structured render and lets R3 simplify the prompt).
  R2 + R3 after R1. R4 (presets) is mostly independent; R5 UX after R4 (needs the
  preset model). R6 last (depends on R4 binding).
- Supersedes the earlier `chatbackend-pane-parity.md` D1-D4 (preset UX) and folds
  P3's bubble into R5's relocation; P5 (diff highlight) / P6 (VRAM confirm) stay as
  their own tracks.
- Boundary discipline: R1 parser is GENERIC (plugin passes the schema); presets +
  prompts + tool set stay plugin-side; core only gains the generic
  `host.toolcall.parse` + the existing capability seams. Audit the new host fn.
- Ties to the AS auto-advance plan's "robust parsing, no false positives, golden +
  negative corpus" discipline — same testing philosophy.

<!-- revision: 1 -->

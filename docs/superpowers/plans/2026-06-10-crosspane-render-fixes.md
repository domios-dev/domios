# Cross-pane chat render fixes (2 small, well-pinpointed bugs)

Both are DRY/centralization slips exposed by per-provider differences. Fix at the
ONE central place, not per provider. Branch off `security` in `C:/Developer/codeterm`.
unit tests next to each file. Verify in a dev build.

---

## Task A — opencode shows raw `-=-codeterm:pane…-=-` markers (claude/codex parse fine)

**Root cause (confirmed):** `packages/chat-core/src/markers.ts:52` `parseMarker`
anchors strictly on the start:

```ts
if (!text.startsWith(MARKER_OPEN)) return null;   // MARKER_OPEN = "-=-codeterm:"
```

The Windows send path prepends a sacrificial space to bracket-paste
(`\x1b[200~ {text}` in `src-tauri/src/service/pty_io.rs`). claude/codex TUIs trim
leading whitespace before storing the user turn; **opencode keeps it**, so its
structured-chat content is `" -=-codeterm:pane …-=-body"` — a leading space →
`startsWith` fails → `null` → the raw marker renders. (Reproduced: opencode chat
content comes back as `" hi"`, `" what is 2+2"` — leading space present.)

**The parser is centralized; its INPUT just isn't normalized. Fix the centralized
parser so it's provider-agnostic:**

In `packages/chat-core/src/markers.ts`, `parseMarker`:
```ts
export function parseMarker(text: string): ParsedMarker | null {
  const t = text.replace(/^\s+/, "");          // tolerate a leading sacrificial space
  if (!t.startsWith(MARKER_OPEN)) return null;
  const rest = t.slice(MARKER_OPEN.length);
  // …rest unchanged, operate on `t`/`rest`…
}
```
The `payload` is the text AFTER the close marker, so trimming the lead doesn't
touch the message body — only the whitespace before the marker.

Also check `src/lib/chatPrefixes.ts` `splitChatPrefixedMessages`: it scans for
`"-=-codeterm:"` at each index, so a single leading space is already handled
there — but it then calls `parseMarker(chunk)`, so the parser fix covers it too.
No per-provider change needed.

**Test:** in `packages/chat-core` (find the markers test, or add one):
`parseMarker(" -=-codeterm:pane label=A pane=abc model=Opus-=-hi")` must return
`{ kind:"pane", attrs:{label:"A",pane:"abc",model:"Opus"}, payload:"hi" }`.
Keep the existing no-leading-space cases green.

---

## Task B — "CodeTerm send" card doesn't show the recipient (кому)

**Root cause:** the chat renders `codeterm send` as a branded card via
`packages/chat-ui/src/codeterm/templates.ts`. The template that prints
`Sent to pane <pane>` is keyed **`"send-text"`** (line 28-34):
```ts
"send-text": (inv, result) => ({ icon:"→", title:`Sent to pane ${pane(inv)}`, … }),
```
But the CLI command was renamed `send-text` → `send` (alias kept). `renderTemplate`
looks the template up by the subcommand path (`"send"`), which no longer matches
`"send-text"`, so `codeterm send … --pane X` falls through to the generic render
— "CodeTerm send ✓ delivered" with no recipient.

**Fix:** key the template on `"send"` and keep `"send-text"` as an alias so both
forms render the recipient. In `templates.ts`:
```ts
const sendTemplate: TemplateFn = (inv, result) => ({
  icon: "→",
  title: `Sent to pane ${pane(inv)}`,   // pane(inv) reads inv.flags.pane
  detail: inv.positionals[0],
  status: deliveredStatus(result),
});
const TEMPLATES: Record<string, TemplateFn> = {
  "send": sendTemplate,
  "send-text": sendTemplate,
  …
};
```
`pane(inv)` (templates.ts:14) already reads `inv.flags.pane`, and the recognizer
already parses `--pane X → { pane:"X" }` (recognize.ts:7). So once the key matches,
the recipient shows for all providers (the card is provider-agnostic — it renders
the agent's `codeterm send` Bash tool-call, same path for claude/codex/opencode).

**Test:** `packages/chat-ui/src/codeterm/recognize.test.ts` (or templates test) —
assert `renderTemplate(recognize('codeterm send "hi" --pane Keats')).title ===
"Sent to pane Keats"`.

---

## Done-criteria
- opencode pane chat: an incoming cross-pane message renders as the "from <Name>"
  origin header + bubble, not the raw `-=-codeterm:…-=-` text.
- A `codeterm send … --pane X` shows "Sent to pane X" for every provider.
- New/updated unit tests green; `pnpm test:plugins` (or the chat-core/chat-ui test
  runner) passes; quick dev-build smoke.

## Security branch implementation (applied)
- Task A: `packages/chat-core/src/markers.ts:52-56` normalize leading whitespace via
  `text.replace(/^\s+/, "")` before `startsWith(MARKER_OPEN)`, then parse as before.
- Task A tests: `packages/chat-core/src/markers.test.ts:1-31` (leading-space marker parse +
  payload-preservation check).
- Task B: `packages/chat-ui/src/codeterm/templates.ts:28-37` add reusable
  `sendTextTemplate` and register it under keys `send` and `send-text`.
- Task B tests: `packages/chat-ui/src/codeterm/recognize.test.ts:40-46` assert renamed
  `send` path still renders `Sent to pane <pane>`.
- Task B manifest coverage: `packages/chat-ui/src/codeterm/template-paths.ts:4-6` add
  `["send"]` alongside `["send-text"]` so manifest assertion includes canonical path.

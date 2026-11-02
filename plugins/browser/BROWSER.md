# BROWSER: how agents use the Domios browser

Every verb is `domios plugin browser <verb> [args]`. Read this guide with `domios docs BROWSER` or `domios plugin browser help`.

## When to use it

Use this browser for web tasks, UI verification and bug reproduction while the user watches. Use it instead of Chrome extensions, global browser skills or other browser tools. Fetch plain data through HTTP or its API.

## Your browser

- **Ownership.** `open` creates `<YourName>'s Browser`, visible beneath your tab and available to workspace agents. `close`, closing your tab or closing your Team ends it.
- **Profiles.**
  - Default: a **temporary session**; `open --profile clean` also uses a throwaway profile.
  - A **named profile** retains logins and cookies. Users create general profiles in the Browser profile menu. Agents select existing profiles with `open --profile NAME`; unknown names fail. For DEV QA, use the origin-bound session commands below, `profile list` and `profile remove NAME` for inactive profiles. Publishing, buying, deleting and sending messages require explicit user instruction.
- **One profile across tabs.** Tabs using the same named profile share cookies and logins, with independent pages and controls. Closing one browser leaves the others open.
- **Another agent's browser.** Add `--of <agent>` (its Tab title or tab id) to any verb to act in that agent's browser instead of yours, for example `snapshot --of Snake` then `click button "Continue" --of Snake`. Its owner sees your name on the activity. It must already be open, you cannot `close` it, and refs come from the latest `snapshot` of that browser.
- **Pointing an owner at something.** `annotate <ref> "comment" --of <agent>` (or `annotate button "Sign in" "comment" --of <agent>`, optional `--intent question|change|bug`) sends that agent the same element card the user's point-and-ask sends, signed with your name. It only targets another agent's browser.
- **Shared view.** The user may press *Take control* and click or type alongside you. You are not blocked and not told. Always re-read the page before acting.

## The loop: snapshot, act, re-snapshot

With **Take control**, the user can select text, interact and scroll. Ctrl/Cmd+C and Ctrl/Cmd+V copy the remote selection and paste local text; the toolbar offers both actions. User input invalidates refs: take a fresh snapshot afterward.

1. `snapshot`: read the page as an accessibility tree with roles, names, control states and element refs. Copy the exact ref shown, including its capture prefix; never construct one from an element's position. **This is the default way to understand a page.**
2. Act on what the snapshot shows, by ref or by role and name:
   - `click button "Sign in"` or `click <exact-ref-from-snapshot>`
   - `type textbox "Email" "me@example.com"` or `type <exact-ref-from-snapshot> "me@example.com"`
   - `press Enter`, `press a ctrl`
   - `goto <url>`, `back`, `forward`, `reload`, `scroll 800`
3. Run `snapshot` again after every navigation or visible change. Elements you saw before may be gone or changed.

Rules:
- Start with `status`. If no browser is open, run `open` before concluding the browser is unavailable.
- Re-snapshot after navigation, clicks that change the page, and any stale-reference error. Every new snapshot issues new refs; full/SPA navigation and viewer input invalidate the previous capture. Navigation, viewer input or a newer capture during a snapshot make the earlier capture fail rather than publishing its refs.
- Role/name locators prefer an exact name, then a substring. If multiple elements match, the command refuses to choose and lists candidate refs; use the intended element's exact ref.
- Control states include `checked`, `selected`, `expanded`, `disabled`, `required`, `readonly`, `focused` and `invalid` when the page exposes them. Use these as evidence when checking a form or toggle.
- `snapshot [limit]` defaults to 200 lines and is capped at 2000. A truncation notice means more content remains; refs are only issued for included elements.
- Prefer refs and role + name from the snapshot over coordinates or CSS selectors.
- After async page changes, wait on evidence (`wait --text T`, `wait --url U`, `wait --selector S`), never on a bare timeout. A wait fails after 15 seconds.
- For secrets, pipe the exact text to `domios plugin browser type <target> --stdin`. Browser masks the input before insertion and redacts the value from output, snapshots, events and annotations. This requires an HTML input that supports password masking. Never put secrets in arguments or print them.
- `type` focuses the element, clears it, then inserts the text. If a custom input still ignores it, click it first and retry.
- Use `text [limit]` to read long content; use `screenshot [file] [--full]` **only for visual claims** (layout, colour, clipping, alignment). Screenshots go to your pad; show one to the user with `<ds-image src="ABSOLUTE_PATH" />`.
- **Never report an action as done unless the page proves it.** Re-snapshot and quote the evidence: a new heading, a success message, the changed value. If an action could have sent, submitted, bought or deleted something and you could not read back the result, say the effect is unproven.
- If a command fails, read its error and retry with corrected arguments when the error is actionable. Don't switch to another browser tool (global browser skills, standalone Playwright, a REPL) just because the first call failed.
- Report flaky behaviour as flaky, not as a pass.

## When the page needs the user

Stop when `status` reports `needs human` other than `none`: captcha, missing login, consent you cannot decide, native file picker, permission prompt or JavaScript dialog. Name the blocker; the user can use *Take control*. Re-snapshot before continuing.

## The user can point at things

User annotations arrive as Domios notices. `inbox` lists unread/undelivered annotations; `inbox all` lists all retained ones. Each includes the page, element role/name/selector, React component (and dev source file), text, attributes, styles, box, nearby text, cropped screenshot and user intent. Find the corresponding code, answer or change it, then verify in the browser.

## Testing your own web app

Reuse the app's dev server or start one in a `--purpose dev` utility shell. `goto http://localhost:<port>` and walk the changed journey, snapshotting each step. Prove the end state (for example, the item persists after reload). Save and cite screenshots for visual claims.

## Other verbs

- `profiles`: the current profile and the named ones available.
- `endpoint`: the browser's local CDP endpoint, for tools that need it.
- `close`: close your browser.

## Device viewport and capture metrics

`viewport set 390 844 --mobile --dpr 2` applies mobile device metrics to your owned target. Attached application targets refuse device emulation. Viewer resizing preserves the mobile mode.

`screenshot phone.png --viewport 390x844 --mobile --dpr 2 --restore --metrics-json` captures at the requested size and reports the saved path, layout width, content width and CDP layout metrics as JSON. `--restore` restores the previous device metrics even when capture fails; restoration failure is reported explicitly. Without `--restore`, requested metrics stay active.

## Scoped atomic actions

Direct and scoped clicks report either synchronous action acknowledgement, same-document navigation, or a main-frame navigation loaded (or restored from the back/forward cache). Action acknowledgement confirms the input/CDP action reply, including synchronous handlers; it does not settle later asynchronous effects. Use an explicit state wait for those effects. Once navigation is observed during the action, completion requires that main frame's matching loader, never network silence. Goto and history commands require their own navigation evidence; an absent history entry reports that no navigation was performed.

`action_unconfirmed` means the action may have occurred but its completion could not be established before the deadline, page closure, observation loss, or a browser authority change. Inspect the current page before continuing and never automatically replay the action.

`click --within 'dialog:Settings' --role button --name Save` and `fill --within 'form:Composer' --role textbox --name Message --text 'hello'` resolve exact role/name matches from a fresh accessibility snapshot at action time. Missing, ambiguous, stale, hidden and disabled targets refuse with a reason code. The page performs the action in one JavaScript turn only if its document and mutation/input epoch still match the capture. An `action_unconfirmed` result means the action may have happened: inspect the page, never replay it.

## Bounded structured snapshots

`snapshot --json --scope dialog --actions-only --max-items 30` returns a JSON object with `page`, `scope`, `items`, `truncated` and `maxItems`. Items contain role, bounded name/text, depth and an optional reference. Only included actions receive references. The maximum is 2000 items; `truncated: true` explicitly marks omitted content.

Scopes select one subtree: `dialog`, `sidebar` (complementary landmark), `composer` (form named Composer), or an exact `role:name`. Ambiguous scopes refuse. Password/token inputs and `data-sensitive` subtrees have their values and names redacted in text and JSON snapshots. A failed DOM redaction capture refuses the snapshot.

## Named DEV instance sessions and cleanup

`open --profile dev-qa --origin http://localhost:7686 --expect-build FULL_SHA` creates or reuses a profile bound to that exact origin, opens its `/web/` UI, and refuses a different origin or an unexpected `/api/health` build SHA. `attach` takes the same options and reuses the owned named session. Origins must be bare HTTP(S) origins without credentials, paths, queries or fragments. Existing unbound user profiles cannot be adopted as per-instance profiles.

`connect --profile dev-qa --origin http://localhost:7686 --expect-build FULL_SHA --stdin` opens the session and accepts the instance API token through piped stdin. Never place the token in argv, a URL or a log. The helper stores and reads back the origin-specific token, then reloads; use snapshot evidence to verify authentication. It never reads an inherited host endpoint or token.

`--ephemeral` on open/attach/connect creates a session profile deleted when its last browser closes. An existing persistent profile cannot become ephemeral. `profile list` lists named profiles; `profile remove dev-qa` refuses while a browser still uses it.

At the end of a QA run, close ephemeral sessions. For a persistent per-instance session, run `open --profile clean`, then `profile remove dev-qa`, then `close`. Remove each per-instance profile used by the run.

`upload <ref|"button text"> <path>...` selects files on Windows without opening an OS dialog. Use an exact file-input ref or the upload button's text from the latest snapshot. Relative paths resolve inside the calling agent's pad, including with `--of`; absolute paths must stay inside that pad or an upload folder the owner granted to the active named profile. The upload activity names the granting folder. The owner manages grants in Settings > General > Browser upload folders or with an owner-authenticated config CLI/API call. Agent credentials cannot add or remove grants. Symlinks, reparse points, device names, parent traversal, directories and alternate data streams refuse. Other platforms report `unsupported` until secure file admission is available.

From an owner-authenticated shell, set the complete per-profile folder map (an empty map removes all grants):

```bash
domios config set browser_upload_folders '{"work":["C:/Developer/domios-promo"]}'
```

The equivalent owner-authenticated request is `PATCH /api/config` with `{"key":"browser_upload_folders","value":{"work":["C:/Developer/domios-promo"]}}`. Changes apply to subsequent uploads without restarting the browser.

The live view shows filenames, byte sizes and the input's target origin. Successful selection reads back the input's file list; it does not establish server upload completion. Re-snapshot and verify the page's result. `upload_unconfirmed` means selection or upload may have happened: inspect the page before another attempt. Up to 64 files may be selected per browser lifetime. Files stay read-locked until that browser closes; close it to release them.

Native `file-picker` driving remains unsupported. Use `upload` before opening a chooser; an already open native dialog requires Take control.

## Accessible state waits

`wait --role button --name 'Aria-only label' --state enabled --timeout 15s` uses Chromium's accessibility tree and its computed accessible names, including aria-only labels. States are `visible`, `hidden`, `enabled` and `expanded`. Hidden also succeeds when no matching accessible element remains; ambiguous matches refuse. Durations use `ms`, `s` or `m`, default to 15s, and are capped at 2m. DOM mutation and input events wake the check; elapsed time only bounds failure. Navigation during a pending wait returns `stale`.

## Disclosure controls

Snapshots issue actionable refs for Chromium `DisclosureTriangle` controls (`summary` is a role alias). `expand summary 'Details'` and `collapse summary 'Details'` capture the current control, click only when its expanded state differs, then re-read the accessibility tree to confirm the requested state. Already matching state succeeds without a click. Ambiguous or stale controls refuse; `action_unconfirmed` or `state_unconfirmed` never triggers a replay.

## QA convenience commands

`open --domios-instance qa --api-port 7790 [--ephemeral]` checks the loopback daemon's exact instance name, API port and DEV application identifier, reads that named DEV instance's protected local credential internally, and connects using an automatically named origin-bound profile. The caller supplies no token. It refuses production port 7685, missing credentials and identity mismatches. Names are lowercase slugs. This convenience path uses the standard named DEV data directory and ignores inherited data-directory/host/token overrides; custom data roots can use the explicit named-session and stdin helper above.

`snapshot --with-text --max-items 80` adds bounded static accessibility leaf text, such as description values and list-item text. It can accompany `--json`, `--scope` and `--actions-only`; static text gets no action refs and sensitive subtrees stay redacted.

`wait --role switch --name Ligatures --state checked` and `--state unchecked` wait for explicit AX checked state. `pressed`, `unpressed`, `selected` and `unselected` use their corresponding AX properties. Missing or mixed values satisfy neither positive nor negative predicates.

`snapshot --json --with-text --out evidence/page.json` and `text 12000 --out evidence/page.txt` save redacted output directly into the calling agent's pad and return its path. Relative paths resolve against the caller's pad, including with `--of`. Absolute paths must remain inside that pad. Parent traversal, symlinks/reparse points and Windows device names refuse.

## Blueprint node

`uses: browser.read_page` reads one page in a Blueprint run, using a separate browser visible under the owning Tab. The node closes it on success or failure. Calls use the exact host-granted surface identity; existing owner browsers stay untouched.

- `with:` takes `url` (http or https, at most 2048 characters), an optional `wait_text`, and `limit`, the number of text lines to keep (default 200, at most 2000). A bound `url` input from an earlier node wins over `with.url`.
- Outputs: `text` (the page text, with credentials redacted) and `final_url`. Bind them downstream as `read.out.text` and `read.out.final_url`.
- Outcomes:
  - `read`: the page was read.
  - `navigation_failed`: the URL is invalid, navigation or `wait_text` failed, or the step deadline was too short.
  - `needs_human`: `status` reported a captcha, login or dialog.
  - `browser_busy`: the step already holds a surface and cannot open another.
- Owner browser state is private: an identity-free step `status` reports only the step's own surface state.

On every outcome other than `read`, `text` holds the reason.

<!-- revision: 14 -->

# BROWSER: how agents use the Domios browser

Every verb is `domios plugin browser <verb> [args]`. This guide is what `domios docs BROWSER` and `domios plugin browser help` print.

## When to use it

- Verify a web UI you changed in the real running app, instead of claiming it works.
- Reproduce a bug a user reported on a web page, and collect evidence.
- Do a web task the user asked for, such as reading a page or filling a form, while the user can watch.

This is the browser for any web page task in Domios. Use it instead of Chrome extensions, global browser skills or other browser tools.

Do not use it to fetch plain data an HTTP request or an API returns. Use `curl` or the API for that.

## Your browser

- **Ownership.** `open` gives you your own browser. It is named `<YourName>'s Browser`, the user sees it live as a tab nested under yours, and other agents in your workspace may use it too. It closes when you close it with `close`, when your Tab closes, or when your Team closes.
- **Profiles.**
  - Default: a **temporary session**. Opening an agent browser does not create a named persistent profile.
  - An explicit `open --profile clean` uses a throwaway profile.
  - A **named profile** keeps logins and cookies across restarts. The user creates general profiles through the Browser profile menu. Agents may select existing profiles with `open --profile NAME`; an unknown name fails. For DEV QA, agents create origin-bound profiles with the named session commands below, list them with `profile list` and remove inactive profiles with `profile remove NAME`. Never publish, buy, delete or send messages without the user's explicit instruction.
- **One profile across tabs.** Tabs using the same named profile share cookies and logins, with independent pages and controls. Closing one browser leaves the others open.
- **Another agent's browser.** Add `--of <agent>` (its Tab title or tab id) to any verb to act in that agent's browser instead of yours, for example `snapshot --of Snake` then `click button "Continue" --of Snake`. Its owner sees your name on the activity. It must already be open, you cannot `close` it, and refs come from the latest `snapshot` of that browser.
- **Pointing an owner at something.** `annotate <ref> "comment" --of <agent>` (or `annotate button "Sign in" "comment" --of <agent>`, optional `--intent question|change|bug`) sends that agent the same element card the user's point-and-ask sends, signed with your name. It only targets another agent's browser.
- **Shared view.** The user may press *Take control* and click or type alongside you. You are not blocked and not told. Always re-read the page before acting.

## The loop: snapshot, act, re-snapshot

The user can press **Take control** to drag-select page text, interact with forms and scroll. Ctrl/Cmd+C copies the remote selection and Ctrl/Cmd+V inserts local clipboard text. The toolbar also has Copy selected text and Paste text actions. User input invalidates agent refs, so take a fresh snapshot before continuing after the user interacts.

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
- Use `text [limit]` to read long content; use `screenshot [file] [--full]` **only for visual claims** (layout, colour, clipping, alignment). Screenshots go to your pad; show one to the user with `@ct-image <path>`.
- **Never report an action as done unless the page proves it.** Re-snapshot and quote the evidence: a new heading, a success message, the changed value. If an action could have sent, submitted, bought or deleted something and you could not read back the result, say the effect is unproven.
- If a command fails, read its error and retry with corrected arguments when the error is actionable. Don't switch to another browser tool (global browser skills, standalone Playwright, a REPL) just because the first call failed.
- Report flaky behaviour as flaky, not as a pass.

## When the page needs the user

`status` shows `needs human` other than `none`, and you should stop and tell the user, when:
- the page shows a bot check or captcha;
- a login is required that you were not given;
- a cookie or consent dialog you may not decide on;
- a native file picker or permission prompt;
- a JavaScript dialog.

Say exactly what is blocking. The user can solve it with *Take control*; then re-snapshot and continue.

## The user can point at things

The user can pick an element in the live view and send a question, change request or bug about it. It arrives in your chat as a Domios notice. If you missed it, `inbox` lists the ones not delivered or read yet (`inbox all` lists every kept one). Each annotation gives you:
- the page;
- the element's role and name, its selector, and its React component (and source file on dev builds);
- its text, attributes, computed styles and box;
- nearby text;
- a cropped screenshot;
- the user's words and intent.

Treat it as a precise pointer: find the code behind the element, answer or change it, then verify in the browser.

## Testing your own web app

1. Start the app's dev server in a `--purpose dev` utility shell, or reuse a running one.
2. `goto http://localhost:<port>`.
3. Walk the user journey you changed, snapshotting after each step.
4. Prove the end state, not just the click. For example, the item appears in the list after reload.
5. Save screenshots for the visual parts and cite them in your report.
6. For each check that passed by hand, propose an automated test (file, framework, assertion).

## Other verbs

- `profiles`: the current profile and the named ones available.
- `endpoint`: the browser's local CDP endpoint, for tools that need it.
- `close`: close your browser.

## Device viewport and capture metrics

`viewport set 390 844 --mobile --dpr 2` applies mobile device metrics to your owned target. Attached application targets refuse device emulation. Viewer resizing preserves the mobile mode.

`screenshot phone.png --viewport 390x844 --mobile --dpr 2 --restore --metrics-json` captures at the requested size and reports the saved path, layout width, content width and CDP layout metrics as JSON. `--restore` restores the previous device metrics even when capture fails; restoration failure is reported explicitly. Without `--restore`, requested metrics stay active.

## Scoped atomic actions

`click --within 'dialog:Settings' --role button --name Save` and `fill --within 'form:Composer' --role textbox --name Message --text 'hello'` resolve exact role/name matches from a fresh accessibility snapshot at action time. Missing, ambiguous, stale, hidden and disabled targets refuse with a reason code. The page performs the action in one JavaScript turn only if its document and mutation/input epoch still match the capture. An `action_unconfirmed` result means the action may have happened: inspect the page, never replay it.

## Bounded structured snapshots

`snapshot --json --scope dialog --actions-only --max-items 30` returns a JSON object with `page`, `scope`, `items`, `truncated` and `maxItems`. Items contain role, bounded name/text, depth and an optional reference. Only included actions receive references. The maximum is 2000 items; `truncated: true` explicitly marks omitted content.

Scopes select one subtree: `dialog`, `sidebar` (complementary landmark), `composer` (form named Composer), or an exact `role:name`. Ambiguous scopes refuse. Password/token inputs and `data-sensitive` subtrees have their values and names redacted in text and JSON snapshots. A failed DOM redaction capture refuses the snapshot.

## Named DEV instance sessions and cleanup

`open --profile dev-qa --origin http://localhost:7686 --expect-build FULL_SHA` creates or reuses a profile bound to that exact origin, opens its `/web/` UI, and refuses a different origin or an unexpected `/api/health` build SHA. `attach` takes the same options and reuses the owned named session. Origins must be bare HTTP(S) origins without credentials, paths, queries or fragments. Existing unbound user profiles cannot be adopted as per-instance profiles.

`connect --profile dev-qa --origin http://localhost:7686 --expect-build FULL_SHA --stdin` opens the session and accepts the instance API token through piped stdin. Never place the token in argv, a URL or a log. The helper stores and reads back the origin-specific token, then reloads; use snapshot evidence to verify authentication. It never reads an inherited host endpoint or token.

`--ephemeral` on open/attach/connect creates a session profile deleted when its last browser closes. An existing persistent profile cannot become ephemeral. `profile list` lists named profiles; `profile remove dev-qa` refuses while a browser still uses it.

At the end of a QA run, close ephemeral sessions. For a persistent per-instance session, run `open --profile clean`, then `profile remove dev-qa`, then `close`. Remove each per-instance profile used by the run.

Native file picker driving returns `unsupported` on all platforms, including Windows: the current host has no lifecycle-owned native dialog handle or file chooser interception. Use Take control to choose files in a dialog the page opened.

<!-- revision: 4 -->

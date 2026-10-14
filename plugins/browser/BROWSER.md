# BROWSER: how agents use the Domios browser

Every verb is `codeterm plugin browser <verb> [args]`. This guide is what `codeterm docs BROWSER` and `codeterm plugin browser help` print.

## When to use it

- Verify a web UI you changed in the real running app, instead of claiming it works.
- Reproduce a bug a user reported on a web page, and collect evidence.
- Do a web task the user asked for, such as reading a page or filling a form, while the user can watch.

This is the browser for any web page task in Domios. Use it instead of Chrome extensions, global browser skills or other browser tools.

Do not use it to fetch plain data an HTTP request or an API returns. Use `curl` or the API for that.

## Your browser

- **Ownership.** `open` gives you your own browser. It is named `<YourName>'s Browser`, the user sees it live as a tab nested under yours, and other agents in your workspace may use it too. It closes when you close it with `close`, when your Tab closes, or when your Team closes.
- **Profiles.**
  - Default: a **persistent owner profile**. Logins and cookies survive closing the Browser tab and restarting Domios. Agent browsers use their owning tab's scope; user browsers use their workspace's scope.
  - An explicit `open --profile clean` uses a throwaway profile.
  - A **named profile** keeps logins and cookies across restarts. It acts as the user. Use one only when the user chose it (`open --profile NAME`), and never publish, buy, delete or send messages without the user's explicit instruction.
- **One profile across tabs.** Tabs using the same named profile share cookies and logins, with independent pages and controls. Closing one browser leaves the others open.
- **Another agent's browser.** Add `--of <agent>` (its Tab title or tab id) to any verb to act in that agent's browser instead of yours, for example `snapshot --of Snake` then `click @e4 --of Snake`. Its owner sees your name on the activity. It must already be open, you cannot `close` it, and refs come from the latest `snapshot` of that browser.
- **Pointing an owner at something.** `annotate @eN "comment" --of <agent>` (or `annotate button "Sign in" "comment" --of <agent>`, optional `--intent question|change|bug`) sends that agent the same element card the user's point-and-ask sends, signed with your name. It only targets another agent's browser.
- **Shared view.** The user may press *Take control* and click or type alongside you. You are not blocked and not told. Always re-read the page before acting.

## The loop: snapshot, act, re-snapshot

1. `snapshot`: read the page as an accessibility tree (roles, names, states, refs like `@e3`). It is compact and cheap. **This is the default way to understand a page.**
2. Act on what the snapshot shows, by ref or by role and name:
   - `click @e7` or `click button "Sign in"`
   - `type @e3 "me@example.com"` or `type textbox "Email" "me@example.com"`
   - `press Enter`, `press a ctrl`
   - `goto <url>`, `back`, `forward`, `reload`, `scroll 800`
3. Run `snapshot` again after every navigation or visible change. Elements you saw before may be gone or changed.

Rules:
- Start with `status`. If no browser is open, run `open` before concluding the browser is unavailable.
- Re-snapshot after navigation, clicks that change the page, and any stale-reference error. Refs from a snapshot are invalidated by navigation.
- Prefer refs and role + name from the snapshot over coordinates or CSS selectors.
- After async page changes, wait on evidence (`wait --text T`, `wait --url U`, `wait --selector S`), never on a bare timeout. A wait fails after 15 seconds.
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

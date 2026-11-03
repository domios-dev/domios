# BROWSER: how agents use the Domios browser

Every verb is `domios plugin browser <verb> [args]`. `help` or `--help` works before open and lists the executing plugin version, host build and verbs. Guide: `domios docs BROWSER`.

## When to use it

Use this browser for web tasks and UI verification while the user watches. Use HTTP/APIs for plain data.

Delegate multi-step flows over 10,000 output characters to a Worker for a short result/evidence/blocker report. Set command/time bounds; stop at bot/auth walls. Workers report to their parent. Bound direct checks with scopes, grep and `--out`.

## Your browser

- **Ownership.** `open` creates `<YourName>'s Browser`, visible beneath your tab and available to workspace agents. `close`, closing your tab or closing your Team ends it.
- **Profiles.**
  - Default: a **temporary session**; `open --profile clean` also uses a throwaway profile.
  - A **named profile** retains logins and cookies. Users create general profiles in the Browser profile menu. Agents select existing profiles with `open --profile NAME`; unknown names fail. For DEV QA, use the origin-bound session commands below, `profile list` and `profile remove NAME` for inactive profiles. Publishing, buying, deleting and sending messages require explicit user instruction.
- **Shared profiles.** Named-profile tabs share cookies/logins; pages and controls stay independent. Closing one leaves the others open.
- **Another agent's browser.** Add `--of <title|tab-id>` to act there. Its owner sees your activity. It must already be open; you cannot close it. Use refs from its latest snapshot.
- **Annotations.** `annotate <ref> "comment" --of <agent>` (or role/name; optional `--intent question|change|bug`) sends a signed element card to another browser's owner.
- **Shared view.** *Take control* lets users act alongside you without notice. Re-read the page before acting.

## The loop: snapshot, act, re-snapshot

With **Take control**, users select text, interact and scroll. Ctrl/Cmd+C/V and the toolbar copy remote selection/paste local text. User input invalidates refs; re-snapshot afterward.

1. `snapshot`: read the page as an accessibility tree with roles, names, control states and element refs. Copy the exact ref shown, including its capture prefix; never construct one from an element's position. **This is the default way to understand a page.**
2. Act on what the snapshot shows, by ref or by role and name:
   - `click button "Sign in"` or `click <exact-ref-from-snapshot>`
   - `type textbox "Email" "me@example.com"` or `type <exact-ref-from-snapshot> "me@example.com"`
   - `press Enter`, `press a ctrl`
   - `goto <url>`, `back`, `forward`, `reload`, `scroll 800`
3. Run `snapshot` again after every navigation or visible change. Elements you saw before may be gone or changed.

Rules:
- Start with `status`; if closed, `open` before declaring the browser unavailable.
- New snapshots replace all refs. Navigation, SPA navigation and viewer input invalidate them. Capture races refuse. Re-snapshot after visible changes or stale errors.
- Role/name matches prefer exact names, then substrings. Name-only `type "Search" "query"` and `fill "Search" "query"` prefer input roles; `click "Search"` prefers clickable roles. An explicit role stays binding. Hidden matches are excluded; exactly one visible match wins. Multiple visible matches refuse with candidate refs.
- Snapshots show control states. Use `snapshot [limit]`, not `--limit`: default 200, maximum 2000; only included elements receive refs.
- Prefer refs or role/name over coordinates/CSS. Wait on evidence with `wait --text T`, `--url U` or `--selector S`, never elapsed time; default deadline 15s.
- Secrets: pipe exact text to `type <target> --stdin`. It requires a maskable HTML input, masks before insertion and redacts output, snapshots, events and annotations. Never put secrets in arguments or print them.
- `type` focuses, clears and inserts, refusing hidden, disabled, readonly or unfocusable fields. `type --submit textbox "Search" "query"` holds the action through typing and Enter. Without navigation or submit evidence, it calls `requestSubmit()` on the focused field's form. Completion requires observed navigation or a submit event; inspect the page for application success. An unconfirmed result never replays typing. `press Enter` sends to the currently focused element.
- `text [limit] [--selector CSS] [--grep TEXT]` reads content. `--grep` keeps lines containing a literal case-insensitive substring before applying the character limit (default 4000, maximum 200000). `--selector` reads the first matching subtree and refuses a missing match. Sensitive subtrees are redacted. Screenshots are for visual claims only; show saved files with `<ds-image src="ABSOLUTE_PATH" />`.
- Prove actions with a fresh snapshot or state wait: changed value, new heading or success message. Unread results remain unproven, especially sends, submissions, purchases or deletions.
- Correct actionable argument errors. Never replay uncertain actions or switch browser tools to bypass a refusal. Report flaky behavior honestly.

## When the page needs the user

Stop when `status` reports `needs human` other than `none`: captcha, missing login, consent you cannot decide, native file picker, permission prompt or JavaScript dialog. Name the blocker; the user can use *Take control*. Re-snapshot before continuing.

## The user can point at things

`inbox` lists unread annotations; `inbox all` lists retained ones. Notices carry page/element/React pointers, text, attributes, styles, box, context, screenshot and intent. Find the code, act and verify.

## Device viewport and capture metrics

Actions show a cursor and typing ring; secrets stay masked. Viewer input hides them without waiting on the page. Evidence omits them unless `--include-overlay` is requested.

`viewport set 390 844 --mobile --dpr 2` applies mobile device metrics to your owned target. Attached application targets refuse device emulation. Viewer resizing preserves the mobile mode.

Screenshot replies report CSS viewport width/height and DPR; recheck coordinates after resizing. `screenshot phone.png --viewport 390x844 --mobile --dpr 2 --restore --metrics-json` adds layout/content/CDP metrics. `--restore` restores prior metrics even after failure; otherwise they stay active.

## Scoped atomic actions

Clicks report synchronous action acknowledgement, same-document navigation or a matching main-frame loader loaded/restored. An acknowledgement includes synchronous handlers; later effects need state waits. Navigation completion never uses network silence. Goto/history require navigation evidence; absent history reports no action. `action_unconfirmed` means completion was lost or authority changed: inspect, never replay.

`click --within 'dialog:Settings' --role button --name Save` and `fill --within 'form:Composer' --role textbox --name Message --text 'hello'` resolve exact role/name matches from a fresh accessibility snapshot at action time. Missing, ambiguous, stale, hidden and disabled targets refuse with a reason code. The page performs the action in one JavaScript turn only if its document and mutation/input epoch still match the capture. An `action_unconfirmed` result means the action may have happened: inspect the page, never replay it.

## Bounded structured snapshots

`snapshot --json --scope dialog --actions-only --max-items 30` returns a JSON object with `page`, `scope`, `items`, `truncated` and `maxItems`. Items contain role, bounded name/text, depth and an optional reference. Only included actions receive references. The maximum is 2000 items; `truncated: true` explicitly marks omitted content.

Scopes select `dialog`, `sidebar` (complementary), `composer` (form named Composer), or exact `role:name`; use `--scope 'main:'` for an unnamed main. Ambiguity refuses. Password/token inputs and `data-sensitive` subtrees are redacted; failed redaction capture refuses.

## Named DEV instance sessions and cleanup

`open --profile dev-qa --origin http://localhost:7686 --expect-build FULL_SHA` creates or reuses a profile bound to that exact origin, opens its root `/` UI, and refuses a different origin or an unexpected `/api/health` build SHA. `attach` takes the same options and reuses the owned named session. Origins must be bare HTTP(S) origins without credentials, paths, queries or fragments. Existing unbound user profiles cannot be adopted as per-instance profiles.

`connect --profile dev-qa --origin http://localhost:7686 --expect-build FULL_SHA --stdin` opens the session and accepts the instance API token through piped stdin. Never place the token in argv, a URL or a log. The helper stores and reads back the origin-specific token, then reloads; use snapshot evidence to verify authentication. It never reads an inherited host endpoint or token.

`--ephemeral` on open/attach/connect creates a session profile deleted when its last browser closes. An existing persistent profile cannot become ephemeral. `profile list` lists named profiles; `profile remove dev-qa` refuses while a browser still uses it.

At the end of a QA run, close ephemeral sessions. For a persistent per-instance session, run `open --profile clean`, then `profile remove dev-qa`, then `close`. Remove each per-instance profile used by the run.

`upload <ref|"button text"> <path>...` selects files on Windows without opening an OS dialog. Use an exact file-input ref or the upload button's text from the latest snapshot. Relative paths resolve inside the calling agent's pad, including with `--of`; absolute paths must stay inside that pad or an upload folder the owner granted to the active named profile. The upload activity names the granting folder. The owner manages grants in Settings > General > Browser upload folders or with an owner-authenticated config CLI/API call. Agent credentials cannot add or remove grants. Symlinks, reparse points, device names, parent traversal, directories and alternate data streams refuse. Other platforms report `unsupported` until secure file admission is available.

From an owner-authenticated shell, set the complete per-profile folder map (an empty map removes all grants):

```bash
domios config set browser_upload_folders '{"work":["C:/Developer/domios-promo"]}'
```

The equivalent owner-authenticated request is `PATCH /api/config` with `{"key":"browser_upload_folders","value":{"work":["C:/Developer/domios-promo"]}}`. Changes apply to subsequent uploads without restarting the browser.

The view shows filenames, sizes and target origin. Selection reads back the file list; verify server completion on the page. `upload_unconfirmed` never authorizes replay. At most 64 files per browser lifetime; closing releases their read locks.

Native `file-picker` driving remains unsupported. Use `upload` before opening a chooser; an already open native dialog requires Take control.

## Accessible state waits

`wait --role button --name 'Aria-only label' --state enabled --timeout 15s` uses computed AX names. States: `visible`, `hidden`, `enabled`, `expanded`. Hidden also accepts no remaining match; ambiguity refuses. Durations: `ms`/`s`/`m`, default 15s, maximum 2m. Mutation/input events wake checks; navigation returns `stale`.

Wait replies identify the matching node or text excerpt and page. Check that evidence: text may match your own chat instruction.

## Disclosure controls

`summary` aliases Chromium `DisclosureTriangle`. `expand summary 'Details'` / `collapse summary 'Details'` click only if needed, then confirm AX expanded state. Already matching state succeeds without a click. Ambiguity/staleness refuse; unconfirmed actions never replay.

## QA convenience commands

`open --domios-instance qa --api-port 7790 [--ephemeral]` verifies the exact DEV name/port/app identifier, reads its protected credential internally and opens `/` in an origin-bound profile. Names are lowercase slugs. Production 7685, missing credentials and identity mismatches refuse. It uses the standard named DEV directory, ignoring inherited directory/host/token overrides; custom roots use explicit sessions and stdin above.

`snapshot --with-text --max-items 80` adds bounded static accessibility leaf text, such as description values and list-item text. It can accompany `--json`, `--scope` and `--actions-only`; static text gets no action refs and sensitive subtrees stay redacted.

`wait --role switch --name Ligatures --state checked` and `--state unchecked` wait for explicit AX checked state. `pressed`, `unpressed`, `selected` and `unselected` use their corresponding AX properties. Missing or mixed values satisfy neither positive nor negative predicates.

`snapshot --json --with-text --out evidence/page.json` and `text 12000 --out evidence/page.txt` save redacted output in the caller's pad. Replies give path, Unicode characters, UTF-8 bytes, items (snapshot entries or text lines) and truncation. Relative/absolute paths stay inside that pad, including with `--of`; traversal, links and device names refuse.

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

## Batch and interactions

`batch --from-pad steps.json` (or piped `--stdin`) runs 1–32 steps in one command. Input (maximum 64K characters) is an array:

```json
[{"name":"snapshot","input":{"args":["--json"]}},{"name":"click","input":{"args":["button","Continue"]}},{"name":"screenshot","input":{"args":["result.png"]}}]
```

Each step uses standalone validation, authority and redaction; put `--of` in its args. Ordered JSON `results` retain saved screenshot paths. `completed: false` and zero-based `stoppedAt` identify the first error or unconfirmed action. Earlier effects remain. Never replay `action_unconfirmed`. No nesting, secret verbs or per-step bodies; use standalone stdin for secrets. Interactions invalidate refs; insert snapshots or use current role/name targets between actions.

- `hover @ref` or `hover button Name`.
- `double-click`, `triple-click`, `right-click`: same target forms.
- `drag @from @to` or `drag x1 y1 x2 y2`: viewport CSS pixels; ref targets must be visible. Native drops require an observed drop; pointer drags require a release.
- `scroll-to @ref`: bring an element into view.
- `key Enter --repeat 3 [shift|ctrl|alt|meta]`: 1–100 repetitions.
- `form-value @ref true|false`: set a checkbox. For a select, pass option value or exact visible text; ambiguous options refuse. Re-snapshot to prove the value.
- `screenshot zoom.png --region x,y,width,height --scale 2`: page CSS coordinates, scale 0.1–4, maximum 16 megapixels. Scale also works without region. Region/full are exclusive; region/scale cannot combine with device viewport options.
- `console [--pattern TEXT] [--limit N]`: literal substring filter, primitive values only; object previews omitted.
- `network [--limit N]`: request/response URL, method and status. Credentials, queries, fragments, headers and bodies are omitted. Both histories retain at most 200 bounded entries per browser session, starting at page open; output defaults to 100, maximum 200.
- `snapshot --ref @ref [--depth N]`: current-ref subtree; cannot combine with `--scope`. Depth 0–64 also applies to full/scoped trees; zero includes only roots. Omitted descendants set truncation and receive no refs.

<!-- revision: 20 -->

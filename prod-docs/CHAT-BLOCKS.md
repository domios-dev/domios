# Chat blocks

Inline references use explicit markers: `<domios type="task" id="task_xxx" plan="pl_xxx" />`,
`<domios type="plan" id="pl_xxx" />`, `<domios type="section" id="sec_xxx" plan="pl_xxx" />`,
or `<domios type="tab" id="TAB_ID" />`. Tasks may omit `plan` when their canonical id
is known. Bare ids remain text; markers inside code remain code. The @ menu inserts markers.

A chat block is a live card for a Domios object. Use a complete typed tag at a
Markdown block boundary, outside code, quotes, lists and context/handoff excerpts.
Several complete tags on separate lines render as stacked cards. Incomplete,
malformed and untrusted tags stay inert. Examples inside code fences stay code.

```text
<ds-guide section="messenger-plugins" />
<ds-plugin id="telegram-client" />
<ds-plugin id="teams-client" />
<ds-connect bridge="telegram" />
```

Attributes are unique lower-case names with quoted values. Escape `&`, `"`, `<`
and `>` in values as entities; entities decode once. JSON arrays carry galleries,
choices, task refs and statuses. Backslashes in path attributes are literal;
backslashes inside JSON follow JSON escaping. No aliases or history rewrites exist.

Every block shares one frame: an icon, a title and a status chip; a one-line
summary with a Details toggle; an actions row; and extras to ask about it in
the composer, pin it to the tab's side panel, and open its full page. The card
reads the real state when it appears and follows it afterwards, so it never
shows a stale snapshot. Showing a card changes nothing:
every action runs only when the user clicks it, risky actions ask first, and
the result appears in the card.

When the user asks what Domios can do, how to reach agents from a phone or a
messenger, or asks you to act in a service whose plugin is not installed, reply
with one lead sentence and the matching blocks rather than long prose. Never
say a plugin is installed when its card shows otherwise.

| Block | When to use |
|---|---|
| `<ds-guide section="SECTION_ID" />` | Point the user at an in-app Guide section that explains a feature or a setup. |
| `<ds-plugin id="PLUGIN_ID" />` | Offer a plugin the user may need, with its install status and an Install, Update or Open button. |
| `<ds-connect bridge="BRIDGE_ID" />` | Show whether a messenger bridge is connected, and let the user connect it in place. |
| `<ds-setting key="plugins.example.enabled" />` | Show one setting with its current value, so the user can change it in place. |
| `<ds-choice options='["Ship it","Hold"]' />` | Ask a question with tappable answers instead of asking the user to type one. |
| `<ds-agent tab="TAB_ID" />` | Show an agent's live state, current task, and Open and Stop controls. |
| `<ds-job ref="JOB_ID_OR_NAME" />` | Show a Cron or Blueprint with its schedule and last result, and Run now, Pause or Resume. |
| `<ds-checklist id="getting-started" />` | Show a setup checklist whose steps tick themselves; `getting-started` for a new user. |
| `<ds-tasks plan="pl_example" />` | Show plan tasks grouped by status, live from the plan. |
| `<ds-canvas>{"items":[…]}</ds-canvas>` | Lay out images, HTML mocks, files and notes side by side in one zoomable board. |
| `<ds-canvas src="ABSOLUTE_PATH.canvas.json" />` | Show a board kept in a file that agents and the owner both edit. |

## `ds-guide`

Renders the Guide section's title and summary with an Open guide action that
opens Settings > Guide, scrolls to the section and focuses its heading. An
unknown id shows a notice and opens the first section. Useful ids include
`overview`, `agents`, `memory`, `supporter-agent`, `mesh`, `blueprints` (turn a
repeated check into a Blueprint), `messengers` (the Telegram bot bridge in
Settings > General > Messengers), `phone` (answer agents
from a Telegram bot) and `messenger-plugins` (let agents use the owner's
Telegram and Teams accounts).

## `ds-plugin`

The id is the plugin id from `domios plugin list` or a channel, such as
`telegram-client`; ids use lowercase letters, digits, `.`, `_` and `-`. The card
shows the plugin's icon, name and one-line outcome, and a status chip:
Installed, Update available or Not installed. Install and Update use the same
path and permissions as the Plugins view and run only on the user's click;
installed plugins with a page offer Open. Open in Plugins shows the full plugin
page. An id that no channel lists renders a small "not found" note, so check
the id before emitting it.

## `ds-connect`

The id names a messenger bridge; `telegram` is the one available today. The
card shows Connected (with the bot it acts as), Connecting, Not responding or
Not connected. When it is not connected, the user pastes the @BotFather token
into the card and presses Connect: the token goes to the secret store, never to
a config file or the chat, and the bridge starts. A saved token connects
without being entered again. Disconnect asks first and keeps the token. Open
Messengers settings shows Settings > General > Messengers, where allowed users
live. Never ask the user to paste a token into the chat; offer this card.

## `ds-setting`

The key is a dot path into the config, the same one `domios config set`
takes, for a setting that a plugin or agent provider declares in its settings
schema: `plugins.<plugin-id>.<key>` or `agents.<provider-id>.<key>`. The card
shows the setting's label and description from the schema and a control for its
type: a switch for a boolean, side-by-side options or a list for a choice, minus
and plus buttons for a number, and a field with Save for text. A change is
written once, through the config set path, when the user clicks; a change made
elsewhere shows in the card. Secrets, tokens, keys and any key the schema does
not declare render a muted "not available" note, so use the setting's exact
schema key.
## `ds-choice`

Put the question in a lead sentence, then a tag such as
`<ds-choice options='["Ship it","Hold","Ask me later"]' />`. Use 2 to 8 distinct
answers of at most 80 characters. JSON escapes preserve literal bars and quotes;
labels normalize whitespace and compare without case. Invalid choices stay inert.
A tap sends that label once, as the user's own reply in this
chat, exactly as if they had typed it; the other answers then disable and the
card shows the choice. A reply the user types instead closes the question. The
card is tied to its message, so it is never pinned, and it sends nothing in any
other view. Act on the reply you receive, not on the card.

## `ds-agent`

The argument is a tab id or its 8-character prefix, as `domios tab list` and
routing markers show it. The card shows the agent's name, role, provider or
model, state (Working, Idle, Waiting for you, Starting or Not running) and the
plan task it is bound to, following them live. Open focuses the tab; Stop, shown
only while the agent works, asks first and then interrupts the current turn
like `domios tab stop`. A tab that has closed renders a "not found" note.

## `ds-job`

The argument is a Cron id or a Blueprint id from `domios cron list` or the
Jobs view; an exact name works when it names one job. The card shows the
schedule, the next run, the last result and the state. A Cron offers Run now,
and Pause or Resume after a confirmation; a running job offers a confirmed
Stop. Blueprints start from their own trigger, so their card offers only Stop
while running. Open in Jobs shows the full job.

## `ds-checklist`

`getting-started` lists first steps: start an agent, connect a phone, install
a plugin, create a Blueprint and save a memory. Each step ticks from real state
when it is done, and an unfinished step offers one click: a deep link, or a
nested block such as `<ds-guide section="phone" />`. Offer it to a new user or when they ask
how to begin.

## `ds-tasks`

Use an explicit plan, with optional typed refs and status arrays:

```text
<ds-tasks plan="pl_example" refs='["COD-1","task_example"]' statuses='["in_progress","blocked"]' />
```

With neither filter, the card shows every task except cancelled ones. Statuses
include `in_progress`, `in_review`, `blocked`, `todo`, `pending`, `paused`, `done`
and `cancelled`; pending and paused group under To do. Rows show executor, model
and progress live and open the task on click; Open plan opens the plan.

In the composer, a complete `<ds-tasks />` may omit the plan only while Domios
resolves the composing tab's Team/workspace plan. Missing scope refuses sending;
the delivered tag always has an explicit plan. The composer expands selection to
a paired snapshot with strict JSON and keeps detached rows if a task disappears:

```text
<ds-tasks plan="pl_example">{"title":"Example","items":[{"id":"task_example","key":"COD-1","status":"pending","name":"A task"}]}</ds-tasks>
```

## `ds-canvas`

A canvas puts several visuals in one frame: before and after screenshots, an
HTML mock beside a note, or design options under named views. It carries its
items in a strict JSON body and lives only in its message:

```text
<ds-canvas>{"title":"Header options","items":[
  {"kind":"image","src":"/absolute/a.png","x":0,"y":0,"w":640,"h":400},
  {"kind":"html","html":"<div class='p-4'>Option B</div>","x":680,"y":0,"w":480,"h":400},
  {"kind":"note","markdown":"**A** is calmer, **B** is denser.","x":0,"y":440,"w":640,"h":120}],
 "views":[{"name":"compare","x":0,"y":0,"w":1160,"h":400}]}</ds-canvas>
```

A canvas has 1 to 64 items. `kind` is `image`, `page` or `document` with an
absolute `src`; `html` with an `html` body that runs in the same sandbox as
`ds-html`; or `note` with `markdown`. Every item may add an `id` (letters,
digits, `_` and `-`), a `title`, and `x`, `y`, `w` and `h` in canvas pixels.
Give `x` and `y` together; items without both flow into rows below the placed
ones at a default size. Up to 16 `views` name rectangles. Unknown fields,
relative paths and duplicate ids or view names make the canvas invalid. The
body is limited to 200,000 bytes, and its file items count toward the message's
32 file previews.

In the chat the canvas shows a static preview about 320px high that never pans
or zooms; click it or press Enter to open the full canvas with Fit, zoom, pan
and a pager that steps through the views, or through the items when there are
none. Click an item to select it: a selected HTML item or document takes the
pointer and wheel, and Escape returns to the canvas. HTML items load only while
visible, and at most 4 run at once; the rest show their title until selected.
An item that cannot load shows one quiet sentence with its cause under Details.
Invalid JSON shows one sentence and the reason under Details. Notes are
Markdown only, so ds tags inside them stay text. Mesh delivery does not
transfer canvas files, so a canvas sent to another machine shows those items
as unavailable.

### Canvas files

A board that should last, and that you and the owner keep changing, belongs
in a file:

```text
<ds-canvas src="/absolute/board.canvas.json" view="compare" />
```

Name the file `NAME.canvas.json`. Don't use a bare `.canvas`, which is another
format. The file's first field is `"format": "domios.canvas/1"`, followed by
exactly the fields of an inline body, up to 200,000 bytes in all:

```json
{
  "format": "domios.canvas/1",
  "title": "Header options",
  "items": [
    {"id": "a", "kind": "image", "src": "/absolute/a.png", "x": 0, "y": 0, "w": 640, "h": 400},
    {"id": "n", "kind": "note", "markdown": "**A** is calmer.", "x": 0, "y": 440, "w": 640, "h": 120}
  ]
}
```

A file without the header first, or with another major version, shows one
sentence with the reason under Details. An inline body takes no header.
`view` is optional and opens the board on that named view. The file follows the same rules as `ds-document`: the tab
must reference it, private paths stay refused, and mesh delivery copies the
file itself but not its items. Its items need no reference of their own, but
private paths still apply to them. The file also appears in the tab's file
list.

Edit the file in place, for example with `jq` or by rewriting it whole. Open
boards update as soon as the file changes, without moving the view. The owner
can move, resize, delete and add notes, and edit note text, in the opened
canvas. Each change is written back into the file at once, keeping the header
and the field order. Ids you omit are written into the file, so give items an
`id` and keep it. Owner edits never
add file items. Read the file again before you rewrite it, because a change
that lands while you rewrite it is lost.

## File previews and HTML

Use `<ds-image src="ABSOLUTE_PATH" />`, `<ds-page src="ABSOLUTE_PATH" />` or
`<ds-document src="ABSOLUTE_PATH" />` at a block boundary. Inline HTML executes
only as `<ds-html>HTML body</ds-html>`; HTML code fences stay copyable code.
Only the complete matching outer close in HTML data state commits the body;
quoted attrs, comments, raw text and inner slash closes cannot terminate it.
Nested ds-html containers are invalid; ds-looking elements inside HTML are inert.

Paths belong to the machine authoring the message. A mesh tab send, delayed send
or mesh-routed agent report transfers these files to the target tab's host
before delivering the message, then replaces file-card path attributes with the returned
target-local paths. A failed transfer refuses delivery; it never reads a
same-named file on the receiving machine. A message may contain at most 32 file
previews. Mesh transfers retain the 25 MiB attachment bound; previews retain
their HTML (200,000 bytes), document (1,000,000 bytes) and image (10 MiB) bounds.
Notification previews require the originating tab identity.

A gallery uses a JSON array of 1 to 32 absolute paths, with exactly one of
`src` or `sources`:

```text
<ds-image sources='["/absolute/path/first image.png","/absolute/path/second image.png"]' />
```

A gallery shows at most 32 images and loads them progressively. Unavailable
images retain a tile with the refusal reason. Fullscreen opens a zoomable canvas
inside the Domios window. Wheel zooms images; plain scroll pans Mermaid.
Ctrl/Cmd+wheel or pinch zooms either viewer, and dragging pans. Double-click or
double-tap fits the set. Previous/next arrows or a horizontal swipe fly to an
image; select an image to focus it, or use Show all to fit the gallery. Late image
loads preserve your chosen view. Reset restores 100% scale. Focus the canvas to
use arrow keys for gallery navigation or diagram pan, +/- to zoom, 0 for 100%,
and Home to fit. Mermaid remains vector artwork at every zoom.

<!-- revision: 10 -->

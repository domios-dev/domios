# Plugin surface tabs

A surface tab is a tab whose content a plugin renders. Domios owns the frame:
the tab's identity, owner, header, lifecycle and restore. The plugin owns what
is inside. Surface tabs sit in the sidebar next to terminals and chats, nested
under the tab that owns them, and render the same way in the desktop app and
the web mirror.

A surface renders in one of two ways. A `view` surface shows the plugin's
sandboxed view (`ui/index.html`), the same page the plugin shows as a panel. A
`stream` surface shows live frames from a core surface runtime, such as the
browser, and the viewer's clicks and keys go back to that runtime.

## Declaring a surface

A plugin declares its surfaces in `plugin.json`:

```json
"capabilities": {
  "view": true,
  "surfaces": [
    { "id": "board", "title": "Board", "renderer": "view", "agentBound": true, "restore": "relaunch" }
  ]
}
```

| Field | Rule |
|---|---|
| `id` | 1-32 lowercase letters, digits or `-`, unique within the plugin |
| `title` | One word without whitespace; it becomes the tab title |
| `renderer` | `view` (the plugin must also declare `capabilities.view`) or `stream` |
| `runtime` | Required for `stream`, refused for `view`: the core surface runtime that renders it. Only `browser` exists |
| `chrome` | Optional, `stream` only: a toolbar spec object core passes to the frontend untouched |
| `agentBound` | Optional. The tab is labelled after its owner, e.g. `Jev's Board` |
| `restore` | Optional: `relaunch` (default), `placeholder` or `none` |
| `icon` | Optional: a Lucide icon name for the tab, lowercase kebab of at most 40 characters (e.g. `globe`); an invalid name is ignored with a warning instead of failing the load. `GET /api/surfaces` carries it on the declared row |

A plugin declares at most 8 surfaces. A manifest that breaks a rule fails to
load, so the plugin never registers a surface it cannot render.

A stream surface needs no view: core branches on the declared `runtime`, never
on the plugin id, and a runtime core does not have refuses the open with
`surface_runtime_unavailable`.

A view surface re-renders on every `restore` value, because a view keeps no
live state of its own. A stream surface with `relaunch` reopens where it was:
the browser runtime reloads its last http(s) or file page in the same profile,
falling back to the clean profile when that profile is gone. `placeholder` and
`none` keep the tab and start the runtime fresh.

The view page receives `surface`, `instance` and `tab` in its query string
(`location.search`) and talks to the plugin through `ct.invoke`, exactly like
the panel view. One plugin view can serve several surfaces by reading
`surface`, and several open copies of one surface by reading `instance`.

## Opening, listing and closing

```bash
codeterm surface list
codeterm surface open dash:board
codeterm surface open dash:board --owner <TAB>
codeterm surface close --tab <TAB>
```

- `surface list` shows the surfaces enabled plugins declare and the surface
  tabs that are open, with their owners. `--json` returns `declared` and `open`.
- `surface open <plugin_id>:<surface_id>` checks that the plugin is installed,
  enabled, declares that surface and can render it (a registered view, or the
  declared stream runtime), then creates the tab and prints its id.
- The owner is `--owner`, otherwise the calling tab when it is an agent tab.
  A plain shell caller opens an ownerless surface. A surface cannot own another
  surface.
- The tab nests under its owner (`parent_tab_id`), opens in the owner's
  workspace and joins the owner's Team through the same membership check as
  `tab new`, so a Team without a live Orchestrator refuses the open. Closing
  the Team closes the surface.
- The tab title is the surface title. The tab header and the sidebar show
  `{Owner}'s {Title}` when the surface is `agentBound` or was opened with
  `--title-from-owner`; the label follows the owner if it is renamed.
- `surface close --tab <TAB>` closes a surface tab and refuses any other tab,
  so it can never close an agent. The tab menu, the header close button and
  `codeterm tab close` close it too.

## Surface tabs take no text

A surface has no terminal and no agent. Every text path refuses it with a typed
code instead of starting a shell:

- `codeterm send`, the `/send` and chat-send routes, and agent-to-agent
  delivery return `surface_no_input`.
- `codeterm tab get-text` and output reads return `surface_no_text`.

Act on a surface through its plugin's verbs (`codeterm plugin --help` lists
installed plugins; each plugin documents its own verbs).

## Lifecycle

- The tab survives restarts and desk restore with its plugin, surface and
  instance ids.
- When the owner tab closes, by any path, its surfaces close with it. Other
  children of the owner are detached as before.
- When the plugin is disabled or uninstalled, the tab stays but its view can
  no longer reach the plugin; close it or re-enable the plugin.
- A stream surface's runtime starts on first use (a viewer connecting or an
  agent verb) and is disposed when the tab closes by any path: `surface close`,
  the owner closing, the workspace closing or the Team closing. The browser
  runtime then closes its Chromium.

## Stream surfaces

A viewer and the agent who owns the surface drive the same runtime instance.
The viewer uses the two routes below; the owner agent uses its plugin's verbs.

### The browser

The `browser` plugin declares `browser:browser`, a stream surface on the
`browser` runtime. Its owner agent drives it with the plugin verbs
(`codeterm plugin browser <verb>`); the plugin's `help` verb prints the
full guide. The verbs act only on the calling agent's own browser, and
`open` creates it when the agent has none. When the user points at an element
and sends a question, change or bug, the owner agent receives it as a Domios
notice; a bare shell owner is refused, and `inbox` returns what was not
delivered or read.

### Plugin access

A plugin reaches its stream surface only through `host.surface.call(verb,
argsJson)`, which returns the JSON text `{ result }` or `{ error }`. It works
only inside `onAgentCommand`, acts on the calling agent's own surface of the
plugin's first stream declaration, and needs the `surfaces` permission, which
the user can revoke like `agentCommands`. `argsJson` may instead be
`{ args, of }`: the core resolves `of` (a tab id or exact agent title) inside
the caller's workspace, refuses mesh-controlled callers, and runs the verb on
that owner's open surface with the caller named as the activity's `actor`.
`annotate` delivers an element card to the surface owner, signed by the caller.

## REST

| Method | Path | Body | Result |
|---|---|---|---|
| GET | `/api/surfaces` | — | `{ declared: [...], open: [...] }` |
| POST | `/api/surfaces/open` | `{ target, owner_tab_id?, caller_tab_id?, workspace_id?, title_from_owner? }` | The opened tab row |
| POST | `/api/surfaces/close` | `{ tab_id, caller_tab_id? }` | `{ tab_id, warnings }` |

| GET | `/api/surfaces/{tab_id}/stream?w=&h=&dpr=` | — | SSE stream of a stream surface |
| POST | `/api/surfaces/{tab_id}/call` | `{ method, args }` | `{ result }` or `{ error }` |

`target` is `<plugin_id>:<surface_id>`. With an owner, `workspace_id` must be
the owner's workspace. Declared rows carry `runtime` and `chrome` for stream
surfaces.

`stream` takes the viewer's CSS size and DPR and applies it before the first
frame. It sends `state` (`{ owner, title, url, activity, needsHuman, profile,
profiles }`) on connect and on every change, `frame` (`{ data, metadata }`,
`data` a base64 JPEG, latest-wins and paced by core; the client never acks)
and `notice` (`{ text, tone? }`). The stream counts as a viewer while it is
open; the last viewer leaving stops the frames. Authenticate with the
`Authorization` header, as with every other route.

`call` methods: `resize { width, height, dpr }`, `input { kind: click | wheel
| text | key, x?, y?, deltaY?, text?, key?, shift?, ctrl?, alt?, meta? }`,
`nav { action: goto | back | forward | reload, target? }`, `screenshot { full? }`
→ `{ base64, path }` (a PNG saved in the owner's pad, or the data dir without
one), `profiles.list` and `profiles.use { name }` (`""` or `clean` is the clean
profile) → `{ current, profiles }`, `grab.hover { x, y }`, `grab.cancel`,
`grab.select { x, y }` → `{ id, label, target }` and `grab.send { id, comment,
intent: question | change | bug }` → `{ markdown, delivered }`, `snip.capture
{ x, y, w, h }` (document CSS px) → `{ id, width, height, thumbnail }` (kept in
memory, nothing written), `snip.cancel { id }` and `snip.send { id, comment,
image? }` → `{ markdown, delivered }` (with `image`, the PNG is saved in the
owner's pad and attached).

Refusals carry `{ ok: false, error: { code, message } }`:

| Code | Meaning |
|---|---|
| `invalid_surface_target` | `target` is not `<plugin_id>:<surface_id>` |
| `surface_plugin_unavailable` | The plugin is not installed and enabled with a view |
| `surface_not_declared` | The plugin declares no such surface |
| `surface_view_unavailable` | The plugin's view is not registered |
| `surface_owner_not_found` | The named owner tab does not exist |
| `surface_owner_is_surface` | The owner is itself a surface tab |
| `surface_workspace_mismatch` | `workspace_id` differs from the owner's workspace |
| `not_a_surface` | `surface close` named a tab that is not a surface |
| `not_a_stream_surface` | `stream` or `call` named a tab that is not a stream surface (HTTP 409) |
| `surface_runtime_unavailable` | The declared stream runtime is not registered in core (HTTP 409) |

<!-- revision: 4 -->

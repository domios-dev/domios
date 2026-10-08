# Plugin surface tabs

A surface tab is a tab whose content a plugin renders. Domios owns the frame:
the tab's identity, owner, header, lifecycle and restore. The plugin owns what
is inside. Surface tabs sit in the sidebar next to terminals and chats, nested
under the tab that owns them, and render the same way in the desktop app and
the web mirror.

Today a surface renders the plugin's sandboxed view (`ui/index.html`), the same
page the plugin shows as a panel.

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
| `renderer` | `view`; the plugin must also declare `capabilities.view` |
| `agentBound` | Optional. The tab is labelled after its owner, e.g. `Jev's Board` |
| `restore` | Optional: `relaunch` (default), `placeholder` or `none` |

A plugin declares at most 8 surfaces. A manifest that breaks a rule fails to
load, so the plugin never registers a surface it cannot render.

Every `restore` value currently re-renders the view when the desk is restored,
because a view keeps no live state of its own. `placeholder` and `none` are
accepted so a manifest can state its intent before other renderers exist.

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
  enabled, declares that surface and has a registered view, then creates the
  tab and prints its id.
- The owner is `--owner`, otherwise the calling tab when it is an agent tab.
  A plain shell caller opens an ownerless surface. A surface cannot own another
  surface.
- The tab nests under its owner (`parent_tab_id`), opens in the owner's
  workspace and joins the owner's Team. Closing the Team closes the surface.
- The tab title is the surface title. The tab header shows `{Owner}'s {Title}`
  when the surface is `agentBound` or was opened with `--title-from-owner`; the
  label follows the owner if it is renamed. The sidebar shows the title nested
  under the owner.
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
- When the owner tab closes, the surface stays open as a top-level tab and its
  label falls back to the title.
- When the plugin is disabled or uninstalled, the tab stays but its view can
  no longer reach the plugin; close it or re-enable the plugin.

## REST

| Method | Path | Body | Result |
|---|---|---|---|
| GET | `/api/surfaces` | — | `{ declared: [...], open: [...] }` |
| POST | `/api/surfaces/open` | `{ target, owner_tab_id?, caller_tab_id?, workspace_id?, title_from_owner? }` | The opened tab row |
| POST | `/api/surfaces/close` | `{ tab_id, caller_tab_id? }` | `{ tab_id, warnings }` |

`target` is `<plugin_id>:<surface_id>`. With an owner, `workspace_id` must be
the owner's workspace. Refusals carry `{ ok: false, error: { code, message } }`:

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

<!-- revision: 1 -->

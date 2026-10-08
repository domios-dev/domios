# Interface conventions

Domios has many surfaces built by many agents: chat, the side panel, the sidebar, Plugins, Settings, Memory, Jobs and Blueprints, notifications and dialogs. They read as one product only when they share a vocabulary. These rules are that vocabulary. A new surface follows them, and a change that touches an existing surface leaves it closer to them.

## Rules

1. **Names, not identifiers.** Primary text is what a person calls the thing: a tab title, a plugin's display name, a provider's name. Ids, absolute paths and CLI command paths never lead a row, card or header. They may appear as a tooltip, a muted secondary line in monospace, or inside a disclosure. When no name is known, the id is the last-resort fallback. A closed or missing entity is described in plain words ("a closed tab"), never by its id.
2. **One action hierarchy.** Buttons come from the shared button set: at most one primary action per surface, quiet (ghost) buttons for row actions, and the danger style only for the destructive confirmation itself. Tags and counts use the shared chip. A surface never invents its own pill or bordered inline button.
3. **Forms read top to bottom.** A field's label sits above its input; help text sits below it in muted type. A command a person might run belongs in a copyable code block inside a disclosure, never in a paragraph of prose.
4. **Time says what it measures.** An age reads as an age ("updated 5m ago"), a duration as a duration ("ran 3m 10s"), a future time as a future ("next in 2h"). A bare "5m" is not allowed, because it cannot be told apart from the other two. The exact timestamp is available on hover.
5. **Counts are compact.** Display counts as 999, 1k, 1.2k, 12k, 1.2M, with the exact number on hover. An exact number is shown in full only where a person edits it.
6. **Status is a word with a dot.** A state is shown as its word with a tone dot or tone chip. Colour never carries a status alone.
7. **One material.** Surfaces use the liquid surface set and the `--ct-*` theme tokens for colour, border, radius and motion. A literal colour appears only as the fallback inside a token reference or in a canvas palette that reads its values from the tokens.
8. **Empty and failed states direct.** An empty or failed state shows one short title and one muted line that says what to do next. It never shows a raw error code or an unstyled paragraph.

## Recurring violations

The table records the classes of inconsistency found in a sweep of every surface, so the same mistakes are recognised the next time they appear.

| Surface | Problem | Rule |
|---|---|---|
| Chat command rows | A `codeterm plugin` call rendered as its CLI path and the plugin id ("plugin get lmstudio") | 1 |
| Chat message origin | A plugin's message header fell back to the plugin id when the message carried no label | 1 |
| Chat removed-plugin state | Two unstyled paragraphs, with the plugin id as the headline | 1, 8 |
| Settings, agent providers grid | Every provider card's subtitle was its id, repeating the name in machine form | 1 |
| Settings, Supporter activity | Request counts printed in full locale format | 5 |
| Sidebar team terminals | "started by" showed the creator's tab id once that tab closed | 1 |
| Sidebar team crons | The report target was named by tab id | 1 |
| Plugins page | Plugin rows carried raw CLI paragraphs | 1, 3 |
| Memory | Ages and durations shared one unlabelled format | 4 |
| Canvas, Usage, Mesh, Titlebar | Literal colours and tracked upper-case labels outside the token set | 7 |
| Plan boards, Usage, delayed sends | Several separate relative-time formatters with different wording | 4 |

## Guards

Where a rule can be checked cheaply it is checked mechanically: bespoke chips and inline buttons are counted against a ratchet that may only fall, ad-hoc spinners and "Loading…" strings are rejected, and every plugin subcommand known to the CLI must render its target by display name in chat. A rule without a guard is enforced by review against this document.

```map
anchors:
  - client:ui
  - capability:chat-view
  - capability:web-ui
  - capability:plugins-and-channels
  - capability:notifications
```

# Sandboxed HTML in chat

Use a fenced `html` block when an interactive visual explains something better
than prose or Mermaid. Ordinary self-contained visuals should render directly;
reading this document is only useful when the block needs Domios-specific APIs.

## Runtime

- HTML, CSS, and JavaScript run in a sandboxed iframe.
- Tailwind CSS v4 utilities, variants, transitions, and animations are bundled.
- `ct.help()` returns the runtime's current capability summary.
- `ct.theme()` returns the current Domios background, foreground, and accent.
- `ct.animate(target, keyframes, options)` wraps Web Animations with bounded
  duration, delay, and iteration counts.
- Direct network and arbitrary filesystem access are blocked.

The source fence is limited to 200,000 characters. Preview content may use
`data:` or `blob:` URLs for images and media, but cannot fetch remote assets.

## File-backed source

For a known absolute local path, use a standalone artifact directive in the
reply. The directive must occupy its own paragraph:

```text
@ds-page C:\Users\User\Desktop\design-system.html
@ds-image C:\Users\User\Desktop\preview.png
@ds-document C:\Users\User\Desktop\notes.md
```

`@ds-page` supports `.html` and `.htm`; `@ds-image` supports PNG, JPEG, GIF,
WebP, BMP, and AVIF; `@ds-document` renders Markdown and safely displays UTF-8
text, logs, JSON, YAML, TOML, and CSV. Domios validates the absolute path,
extension, and size before exposing the bounded file to the renderer. SVG is
excluded from image previews because it can contain active or external content.
Windows paths may use either backslashes or forward slashes; both forms are
preserved exactly when the preview file is resolved.

## Chat blocks

A chat block is a live card for a Domios object. Write each block as its own
line, `@ds-<name> <argument>`, outside code fences, quotes and lists. Several
block lines in a row render as stacked cards; a line with any other text stays
plain text. Inline mentions never become blocks.

```text
@ds-guide messenger-plugins
@ds-plugin telegram-client
@ds-plugin teams-client
```

Every block shares one frame: an icon, a title and a status chip; a one-line
summary with a Details toggle; an actions row; and extras to copy the block
line, ask about it in the composer, pin it to the tab's side panel, and open
its full page. The card reads the real state when it appears and follows it
afterwards, so it never shows a stale snapshot. Showing a card changes nothing:
every action runs only when the user clicks it, risky actions ask first, and
the result appears in the card.

When the user asks what Domios can do, how to reach agents from a phone or a
messenger, or asks you to act in a service whose plugin is not installed, reply
with one lead sentence and the matching blocks rather than long prose. Never
say a plugin is installed when its card shows otherwise.

| Block | When to use |
|---|---|
| `@ds-guide <section-id>` | Point the user at an in-app Guide section that explains a feature or a setup. |
| `@ds-plugin <plugin-id>` | Offer a plugin the user may need, with its install status and an Install, Update or Open button. |

### `@ds-guide`

Renders the Guide section's title and summary with an Open guide action that
opens Settings > Guide, scrolls to the section and focuses its heading. An
unknown id shows a notice and opens the first section. Useful ids include
`overview`, `agents`, `memory`, `supporter-agent`, `mesh`, `integrations`,
`phone` (answer agents from a Telegram bot) and `messenger-plugins` (let agents
use the owner's Telegram and Teams accounts).

### `@ds-plugin`

The id is the plugin id from `codeterm plugin list` or a channel, such as
`telegram-client`; ids use lowercase letters, digits, `.`, `_` and `-`. The card
shows the plugin's icon, name and one-line outcome, and a status chip:
Installed, Update available or Not installed. Install and Update use the same
path and permissions as the Plugins view and run only on the user's click;
installed plugins with a page offer Open. Open in Plugins shows the full plugin
page. An id that no channel lists renders a small "not found" note, so check
the id before emitting it.

## File-backed HTML fences

The earlier fenced HTML form remains supported:

When an HTML file already exists at a known absolute path, keep it as the single
source instead of copying its contents into the reply:

````markdown
```html
@file C:\Users\User\Desktop\design-system.html
```
````

Domios reads the file from the desktop host, removes scripts and other active
or external content, then renders the result in the normal message sandbox. The
toolbar shows `Local file`, the source path, and a Reload action. Returning focus
to the Domios window also reloads the file, so edits are picked up without a
timer or a duplicated HTML payload.

The directive must be the entire fence body. Paths must be absolute, point to a
UTF-8 `.html` or `.htm` file, and stay within the 200,000-byte preview limit.
Use this only for a path already known to the agent; do not read the file merely
to paste its contents into the fence.

## User-selected files

`ct.file.pick(options)` opens a file chooser and exposes only the file the user
explicitly selects. It returns `null` when selection is cancelled, otherwise:

```js
{ name, type, size, data }
```

`data` is a data URL by default. Set `as: "text"` for text or JSON, or
`as: "arrayBuffer"` for binary processing. The hard maximum is 5 MiB;
`maxBytes` may set a smaller limit and `accept` may narrow the picker.

Image preview:

```html
<button id="choose" class="rounded bg-blue-500 px-3 py-2 text-white">
  Choose image
</button>
<img id="preview" class="mt-3 max-h-80 rounded-xl object-contain" alt="" />
<script>
document.querySelector('#choose').onclick = async () => {
  const file = await ct.file.pick({ accept: 'image/*' });
  if (file) document.querySelector('#preview').src = file.data;
};
</script>
```

JSON visualization:

```html
<button id="choose">Choose JSON</button>
<pre id="output"></pre>
<script>
document.querySelector('#choose').onclick = async () => {
  const file = await ct.file.pick({ accept: '.json,application/json', as: 'text' });
  if (file) document.querySelector('#output').textContent =
    JSON.stringify(JSON.parse(file.data), null, 2);
};
</script>
```

Keep the picker behind an explicit user action such as a button click. Never
claim that the block can inspect a path or browse the machine on its own.

## Untrusted local HTML

`ct.file.renderHtml(options)` opens an HTML file and renders its safe static
content inside the current preview. The file never passes through the agent.
Scripts, event handlers, forms, nested browsing contexts, navigation attributes,
external media, CSS imports, and CSS URLs are removed. HTML, safe inline styles,
Tailwind classes, embedded raster/media data URLs, and CSS animations remain.

```html
<button onclick="ct.file.renderHtml()">Open local HTML</button>
```

The rendered document carries a visible `Local untrusted HTML` banner. Use
`target` to replace a specific element instead of the preview body:

```js
ct.file.renderHtml({ target: '#canvas', maxBytes: 2 * 1024 * 1024 })
```

<!-- revision: 8 -->

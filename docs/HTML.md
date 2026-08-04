# Sandboxed HTML in chat

Use a fenced `html` block when an interactive visual explains something better
than prose or Mermaid. Ordinary self-contained visuals should render directly;
reading this document is only useful when the block needs CodeTerm-specific APIs.

## Runtime

- HTML, CSS, and JavaScript run in a sandboxed iframe.
- Tailwind CSS v4 utilities, variants, transitions, and animations are bundled.
- `ct.help()` returns the runtime's current capability summary.
- `ct.theme()` returns the current CodeTerm background, foreground, and accent.
- `ct.animate(target, keyframes, options)` wraps Web Animations with bounded
  duration, delay, and iteration counts.
- Direct network and arbitrary filesystem access are blocked.

The source fence is limited to 200,000 characters. Preview content may use
`data:` or `blob:` URLs for images and media, but cannot fetch remote assets.

## File-backed source

For a known absolute local path, use a standalone artifact directive in the
reply. The directive must occupy its own paragraph:

```text
@ct-page C:\Users\dev\Desktop\design-system.html
@ct-image C:\Users\dev\Desktop\preview.png
@ct-document C:\Users\dev\Desktop\notes.md
```

`@ct-page` supports `.html` and `.htm`; `@ct-image` supports PNG, JPEG, GIF,
WebP, BMP, and AVIF; `@ct-document` renders Markdown and safely displays UTF-8
text, logs, JSON, YAML, TOML, and CSV. CodeTerm validates the absolute path,
extension, and size before exposing the bounded file to the renderer. SVG is
excluded from image previews because it can contain active or external content.
Windows paths may use either backslashes or forward slashes; both forms are
preserved exactly when the preview file is resolved.

The earlier fenced HTML form remains supported:

When an HTML file already exists at a known absolute path, keep it as the single
source instead of copying its contents into the reply:

````markdown
```html
@file C:\Users\dev\Desktop\design-system.html
```
````

CodeTerm reads the file from the desktop host, removes scripts and other active
or external content, then renders the result in the normal message sandbox. The
toolbar shows `Local file`, the source path, and a Reload action. Returning focus
to the CodeTerm window also reloads the file, so edits are picked up without a
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

<!-- revision: 3 -->

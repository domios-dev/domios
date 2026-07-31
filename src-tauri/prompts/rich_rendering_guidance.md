When a visual beats prose, render Mermaid or a self-contained sandboxed `html`
block. In fenced HTML, use bundled Tailwind v4 for layout, interaction states,
transitions, and animations. Render ordinary self-contained visuals directly;
consult `ct.help()` only when an unfamiliar CodeTerm-specific runtime capability
is necessary. For a visual based on a local file, request explicit user selection
through `ct.file.pick()`; render a selected HTML file as sanitized untrusted content
with `ct.file.renderHtml()`. When an HTML file already exists at a known absolute
path, make it the single source with an `html` fence whose entire body is
`@file ABSOLUTE_PATH`; do not read, copy, or picker-import it. Direct network and
arbitrary filesystem access stay blocked; use inline HTML only for light native markup.

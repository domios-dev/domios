# Whiteboard Plugin (tldraw)

**Status (2026-04-13):** Not started — design phase only. Zero code references in src/ or src-tauri/. Estimated ~35–45h to ship first working version.  
**Date:** 2026-04-05  
**Priority:** P2

## What

Interactive canvas inside CodeTerm — agents draw architecture diagrams, flowcharts, mind maps. Humans edit visually. Based on [tldraw](https://github.com/tldraw/tldraw) (MIT, React, headless SDK).

## Why

Agents think in text but architecture is spatial. A whiteboard lets agents:
- Draw system diagrams while planning
- Visualize data flows, component relationships
- Create diagrams for documentation (export SVG/PNG)
- Read back what's on the canvas to reason about it

Users get an interactive canvas they can edit alongside agents.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  CodeTerm                                            │
│                                                      │
│  ┌────────────────────────────────────────────────┐  │
│  │  Browser Pane (WebView)                        │  │
│  │  ┌──────────────────────────────────────────┐  │  │
│  │  │  tldraw React App (thin wrapper)         │  │  │
│  │  │                                          │  │  │
│  │  │  Full tldraw UI: draw, select, zoom,     │  │  │
│  │  │  pan, shapes, arrows, text, frames,      │  │  │
│  │  │  freehand, images, groups                 │  │  │
│  │  │                                          │  │  │
│  │  │  ← postMessage API →                     │  │  │
│  │  └──────────────────────────────────────────┘  │  │
│  └──────────────────┬─────────────────────────────┘  │
│                     │ postMessage / Tauri invoke      │
│  ┌──────────────────▼─────────────────────────────┐  │
│  │  whiteboard plugin (native)                     │  │
│  │                                                  │  │
│  │  • Manages tldraw pane lifecycle                │  │
│  │  • Translates MCP tool calls → tldraw commands  │  │
│  │  • Reads canvas state → structured JSON         │  │
│  │  • Exports SVG/PNG                              │  │
│  │  • Persists snapshots to disk                   │  │
│  └──────────────────┬─────────────────────────────┘  │
│                     │ Plugin API                      │
│  ┌──────────────────▼─────────────────────────────┐  │
│  │  MCP Server (core)                              │  │
│  │                                                  │
│  │  whiteboard.* tools exposed to all agents       │  │
│  └─────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

## tldraw Integration

### Package Structure

```
plugins/whiteboard/
├── plugin.toml              # plugin manifest
├── package.json             # tldraw dependency
├── src/
│   ├── App.tsx              # thin wrapper around <Tldraw />
│   ├── bridge.ts            # postMessage ↔ CodeTerm communication
│   └── agent-api.ts         # programmatic shape/arrow creation
├── prompts/
│   ├── ga_whiteboard.md     # GA prompt section
│   └── skill_whiteboard.md  # /codeterm skill section
└── dist/                    # built assets loaded in WebView
```

### Dependencies

```json
{
  "dependencies": {
    "@tldraw/tldraw": "^3.x",
    "@tldraw/store": "^3.x"
  }
}
```

tldraw updates independently via `npm update`. Our wrapper is minimal — mostly bridge code.

## MCP Tools for Agents

### Canvas Management

```
whiteboard.create
  → Opens a new whiteboard pane in current workspace
  → Returns { pane_id, canvas_id }

whiteboard.open { canvas_id }
  → Opens existing saved canvas

whiteboard.close { pane_id }
  → Closes whiteboard pane

whiteboard.list
  → List saved canvases with names and dates
```

### Drawing — High Level (recommended for agents)

Agents don't need to know tldraw internals. These tools work with semantic concepts:

```
whiteboard.add_box { canvas_id, text, x, y, width?, height?, color? }
  → Creates a labeled rectangle
  → Returns { shape_id }

whiteboard.add_circle { canvas_id, text, x, y, radius?, color? }
  → Creates a labeled circle/ellipse
  → Returns { shape_id }

whiteboard.add_text { canvas_id, text, x, y, size? }
  → Creates a text label
  → Returns { shape_id }

whiteboard.add_arrow { canvas_id, from_shape_id, to_shape_id, label?, color? }
  → Connects two shapes with a labeled arrow
  → Returns { shape_id }

whiteboard.add_frame { canvas_id, text, x, y, width, height }
  → Creates a frame (group container) with a title
  → Returns { shape_id }

whiteboard.add_note { canvas_id, text, x, y, color? }
  → Sticky note
  → Returns { shape_id }

whiteboard.move { canvas_id, shape_id, x, y }
  → Move a shape to new position

whiteboard.delete { canvas_id, shape_ids[] }
  → Remove shapes

whiteboard.group { canvas_id, shape_ids[], label? }
  → Group shapes together
  → Returns { group_id }

whiteboard.style { canvas_id, shape_id, color?, border?, font_size? }
  → Update shape appearance
```

### Drawing — Low Level (for power users)

```
whiteboard.raw { canvas_id, tldraw_command }
  → Pass raw tldraw store command (for anything not covered above)
  → Full tldraw API access
```

### Reading Canvas

This is critical — agents need to understand what's on the canvas:

```
whiteboard.read { canvas_id }
  → Returns structured JSON of all shapes:
    {
      shapes: [
        { id: "s1", type: "box", text: "Auth Service", x: 100, y: 200, width: 150, height: 80 },
        { id: "s2", type: "box", text: "Database", x: 400, y: 200, width: 150, height: 80 },
        { id: "a1", type: "arrow", from: "s1", to: "s2", label: "SQL queries" }
      ],
      frames: [
        { id: "f1", text: "Backend", contains: ["s1", "s2", "a1"] }
      ]
    }

whiteboard.read_text { canvas_id }
  → Returns human-readable text description:
    "Canvas 'System Architecture':
     Frame 'Backend':
       [Auth Service] --SQL queries--> [Database]
       [Auth Service] --HTTP--> [API Gateway]
     Frame 'Frontend':
       [React App] --REST--> [API Gateway]"

whiteboard.screenshot { canvas_id, format? }
  → Returns PNG/JPEG of current canvas view
  → For multimodal agents that can see images

whiteboard.export_svg { canvas_id }
  → Returns SVG string (for docs, embedding)
```

### Templates

```
whiteboard.template { canvas_id, type }
  → Apply a layout template:
    "architecture"  — boxes in layers (frontend/backend/data)
    "flowchart"     — vertical flow with decision diamonds
    "mindmap"       — radial from center
    "sequence"      — vertical timeline with arrows
    "kanban"        — columns (todo/doing/done)
    "blank"         — empty canvas
```

## Agent Interaction Examples

### Agent draws architecture diagram

```
Agent thinking: "Let me visualize the system architecture"

1. whiteboard.create → pane_id: "wb1", canvas_id: "arch-001"

2. whiteboard.add_frame { canvas_id: "arch-001", text: "Frontend", x: 50, y: 50, width: 300, height: 200 }
3. whiteboard.add_box { canvas_id: "arch-001", text: "React App", x: 100, y: 120, color: "blue" } → s1
4. whiteboard.add_box { canvas_id: "arch-001", text: "Next.js SSR", x: 100, y: 200, color: "blue" } → s2

5. whiteboard.add_frame { canvas_id: "arch-001", text: "Backend", x: 450, y: 50, width: 300, height: 200 }
6. whiteboard.add_box { canvas_id: "arch-001", text: "FastAPI", x: 500, y: 120, color: "green" } → s3
7. whiteboard.add_box { canvas_id: "arch-001", text: "PostgreSQL", x: 500, y: 200, color: "orange" } → s4

8. whiteboard.add_arrow { from: "s1", to: "s3", label: "REST API" }
9. whiteboard.add_arrow { from: "s3", to: "s4", label: "SQL" }
```

### Agent reads and reasons about existing diagram

```
Agent: "Let me look at the current architecture"

1. whiteboard.read_text { canvas_id: "arch-001" }
   → "Frame 'Frontend': [React App], [Next.js SSR]
      Frame 'Backend': [FastAPI] --SQL--> [PostgreSQL]
      [React App] --REST API--> [FastAPI]"

2. Agent reasons: "I see the frontend talks to FastAPI via REST.
   We need to add a WebSocket connection for real-time updates."

3. whiteboard.add_arrow { from: "s1", to: "s3", label: "WebSocket", color: "red" }
4. whiteboard.add_note { text: "TODO: Add WebSocket for live updates", x: 350, y: 350, color: "yellow" }
```

### Agent exports for documentation

```
Agent: "Let me add this diagram to the docs"

1. whiteboard.export_svg { canvas_id: "arch-001" }
   → SVG string

2. Write SVG to docs/assets/architecture.svg
3. Reference in README: ![Architecture](docs/assets/architecture.svg)
```

## GA Prompt Section

When whiteboard plugin is active, this gets appended to General Agent prompt:

```markdown
## Whiteboard

You have access to an interactive canvas for visual thinking.

Use `whiteboard.create` to open a canvas, then draw with:
- `whiteboard.add_box` — labeled rectangles (services, components)
- `whiteboard.add_arrow` — connections between shapes (data flow, dependencies)
- `whiteboard.add_frame` — group related shapes (layers, domains)
- `whiteboard.add_note` — sticky notes for TODOs and annotations

Read back with:
- `whiteboard.read_text` — text description of what's on canvas
- `whiteboard.screenshot` — visual snapshot (if you can see images)

When to use:
- Planning architecture before coding
- Explaining system design to the user
- Visualizing data flows or state machines
- Creating diagrams for documentation (export SVG)

Canvas data persists — you can open previous canvases with `whiteboard.open`.
```

## Persistence

- Canvas data saved as tldraw JSON snapshots in `~/.codeterm/plugins/whiteboard/canvases/`
- Named by canvas_id: `arch-001.json`
- Auto-save on every change (tldraw store subscription)
- Mesh sync: canvases can replicate to peers (opt-in per canvas)

## Plugin Manifest

```toml
[plugin]
name = "whiteboard"
version = "0.1.0"
description = "Interactive canvas with tldraw — agents draw diagrams, users edit visually"
tier = "native"
platforms = ["windows", "macos", "linux"]

[capabilities]
tools = [
  "whiteboard.create", "whiteboard.open", "whiteboard.close", "whiteboard.list",
  "whiteboard.add_box", "whiteboard.add_circle", "whiteboard.add_text",
  "whiteboard.add_arrow", "whiteboard.add_frame", "whiteboard.add_note",
  "whiteboard.move", "whiteboard.delete", "whiteboard.group", "whiteboard.style",
  "whiteboard.raw",
  "whiteboard.read", "whiteboard.read_text", "whiteboard.screenshot", "whiteboard.export_svg",
  "whiteboard.template"
]
commands = ["whiteboard"]
settings = true
hooks = ["on_prompt_build"]
pane_types = ["whiteboard"]

[permissions]
filesystem = ["~/.codeterm/plugins/whiteboard/"]

[activation]
triggers = ["on_command:whiteboard", "on_tool:whiteboard.*"]

[mesh]
replicate_state = true

[prompt]
ga_section = "prompts/ga_whiteboard.md"
```

## Implementation Notes

- tldraw v3 has a clean store API — shapes are plain JSON records
- `editor.createShape()`, `editor.getShapes()`, `editor.deleteShapes()` — straightforward
- SVG export: `editor.getSvgString()` built-in
- PNG export: `editor.toImage()` built-in (needs canvas)
- Store subscriptions: `editor.store.listen()` for auto-save
- Collaboration: tldraw has built-in sync protocol (Y.js compatible) — mesh sync could use this later

## Open Questions

- Should whiteboard be a separate pane type or reuse browser pane + tldraw URL?
  - Separate pane type = cleaner, dedicated toolbar, better integration
  - Browser pane = simpler, reuses existing infra
  - **Recommendation:** Start as browser pane (faster to ship), migrate to dedicated pane type later

- Should canvases be per-workspace or global?
  - Per-workspace = cleaner isolation, matches project scope
  - Global = agents can reference any canvas from anywhere
  - **Recommendation:** Global canvases, but workspace can have "pinned" canvases

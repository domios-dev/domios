import type { Section, SpecLink } from "./types";


export const PERF_SPEC_ID = "spec_canvas_substrate";

const canvas = { x: null, y: null, collapsed: false };
const sec = (id: string, title: string, body: string, tags: string[] = [], subsections: Section[] = []): Section => ({
  id,
  title,
  body,
  tags,
  subsections,
  canvas,
});

export const PERF_PLAN_SECTIONS: Section[] = [
  sec(
    "pl_layout",
    "Layout & banding",
    [
      "## Approach",
      "Every plan is laid out **once per structural change**, never per zoom frame. The",
      "packer walks plans → frames → nodes in a single pass and assigns each task a slot",
      "in a banded cloud anchored to its plan, so the cost is `O(N)` in node count and",
      "independent of the camera.",
      "",
      "## Integrations",
      "- Region packing reuses the shared `regionPacking` grid so frames never overlap.",
      "- The scene + layout memos are keyed on a `layoutInputRevision` digest — the",
      "  dependency array deliberately **excludes `cameraScale`** so pan/zoom never",
      "  rebuilds geometry.",
      "",
      "See [Worlds & content-LOD](#pl_worlds) for how a fixed frame reveals its interior.",
    ].join("\n"),
    ["perf", "layout"],
  ),
  sec(
    "pl_worlds",
    "Worlds & content-LOD",
    [
      "## Approach",
      "A node crosses its own content bands (**W0 mass → W3 deep**) by *apparent width*",
      "— `footprintWorldWidth × cameraScale` — not raw camera scale. A wide spec and a",
      "tiny task therefore reveal their interiors at the moment each is physically big",
      "enough to read.",
      "",
      "## Why hysteresis",
      "A node parked on a threshold would flicker as the camera jitters, so each boundary",
      "carries a ±10% dead zone. Geometry is **reserved at the deepest band**: W2→W3",
      "changes only rasterization, never layout, which keeps scroll position stable.",
    ].join("\n"),
    ["worlds", "lod"],
  ),
  sec(
    "pl_document",
    "Plan-as-document",
    [
      "## Approach",
      "A plan section is a **readable document**, not a terse index row. At W2+ the body",
      "renders as full markdown — headings, lists, code, links — inside a node-internal",
      "scroll column, so a long section reads top-to-bottom with nothing clipped.",
      "",
      "## Provenance without wires",
      "Each section shows small **spec-reference bubbles** naming the spec section(s) it",
      "was written from; clicking one flies to that spec section. These bubbles *replace*",
      "node-to-node edges — provenance reads inline instead of as a tangle of curves.",
      "",
      "Rendering delegates to the shared `RichMarkdown` component for parity with chat.",
    ].join("\n"),
    ["reading", "provenance"],
  ),
  sec(
    "pl_liveness",
    "Liveness & presence",
    [
      "## Approach",
      "Presence is **live only**: a Tab bound to a task surfaces as an agent token on",
      "the work, never a stale avatar. The plan node's swarm panel aggregates task status",
      "into a single proportional bar whose in-progress segment pulses.",
      "",
      "## Integrations",
      "- The orchestrator heartbeat (`current_focus`, `active_lanes`) renders when present.",
      "- Loop status reuses the section-row glyph palette — no new colour vocabulary.",
    ].join("\n"),
    ["liveness", "presence"],
  ),
  sec(
    "pl_navigation",
    "Navigation & fly-to",
    [
      "## Approach",
      "One delegated click handler resolves any `data-fly-to-*` chip to a node id and",
      "eases the camera there with an arrival blink. A spec bubble lands on the spec",
      "node and scrolls its target section into view.",
      "",
      "## Return-to-plan",
      "Every dive keeps a home affordance so the reader can always fly back to the plan —",
      "navigation is reversible by construction.",
    ].join("\n"),
    ["navigation", "fly-to"],
  ),
];

export const PERF_PLAN_SECTION_IDS: string[] = PERF_PLAN_SECTIONS.map((s) => s.id);

export const PERF_SPEC_SECTIONS: Section[] = [
  sec(
    "sp_scale",
    "Render at any scale",
    "The board MUST render every plan at any zoom without overlap or dropped frames, even at tens of thousands of tasks.",
    ["requirement"],
  ),
  sec(
    "sp_zoom_nav",
    "Zoom is navigation",
    "Zooming MUST reveal progressively deeper detail: a far glance is a meaningful summary, a near view is complete.",
    ["requirement"],
  ),
  sec(
    "sp_readable",
    "Plans read like documents",
    "A plan MUST be readable like a real document — the reasoning, approaches and integrations visible and scrollable.",
    ["requirement"],
  ),
  sec(
    "sp_traceable",
    "Plans trace to the spec",
    "Every plan section MUST show which specification it derives from, and let the reader jump straight to that spec section.",
    ["requirement"],
  ),
  sec(
    "sp_live",
    "Liveness is first-class",
    "The canvas MUST show who is working right now and on what, with live presence rather than stale state.",
    ["requirement"],
  ),
  sec(
    "sp_fluid_nav",
    "Fluid, reversible navigation",
    "The reader MUST be able to fly fluidly between related nodes and always return to the plan.",
    ["requirement"],
  ),
];

// plan_section_id → spec_section_id(s) it was written from (exercises 1- and 2-bubble rows).
export const PERF_SPEC_LINKS: SpecLink[] = [
  { spec_id: PERF_SPEC_ID, spec_section_id: "sp_scale", plan_section_id: "pl_layout" },
  { spec_id: PERF_SPEC_ID, spec_section_id: "sp_zoom_nav", plan_section_id: "pl_worlds" },
  { spec_id: PERF_SPEC_ID, spec_section_id: "sp_scale", plan_section_id: "pl_worlds" },
  { spec_id: PERF_SPEC_ID, spec_section_id: "sp_readable", plan_section_id: "pl_document" },
  { spec_id: PERF_SPEC_ID, spec_section_id: "sp_traceable", plan_section_id: "pl_document" },
  { spec_id: PERF_SPEC_ID, spec_section_id: "sp_live", plan_section_id: "pl_liveness" },
  { spec_id: PERF_SPEC_ID, spec_section_id: "sp_fluid_nav", plan_section_id: "pl_navigation" },
  { spec_id: PERF_SPEC_ID, spec_section_id: "sp_zoom_nav", plan_section_id: "pl_navigation" },
];
